// LD_PRELOAD interposer that patches the stock ETS2 client's
// ISteamNetworkingSockets vtable so Convoy payloads are dumped in plaintext
// before Steam encrypts them.
//
// Build:
//   gcc -shared -fPIC -O2 -o libnetcap-client.so netcap-client.c -ldl
// Run:
//   SteamAppId=227300 LD_PRELOAD=./libnetcap-client.so \
//     NETCAP_OUT=/tmp/convoy-client.log ./eurotrucks2
//
// The dedicated-server variant hooks SteamInternal_GameServer_Init, which the
// client never calls; this one hooks SteamAPI_Init instead.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

// ISteamNetworkingSockets vtable slots, from the flat wrappers:
//   mov (%rdi),%rax ; jmp *0x58(%rax)  etc.
#define IDX_SEND 11
#define IDX_SENDM 12
#define IDX_RECV 14
#define IDX_RECVP 27 // ReceiveMessagesOnPollGroup -> jmp *0xd8(%rax)

static FILE *out;

static void ensure(void) {
    if (out) return;
    const char *p = getenv("NETCAP_OUT");
    out = fopen(p ? p : "/tmp/convoy-client.log", "a");
    if (out) setvbuf(out, NULL, _IOLBF, 0);
}

static void dump(const char *tag, uint32_t conn, const void *data, uint32_t cb) {
    ensure();
    if (!out || !data) return;
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    fprintf(out, "%ld.%06ld %s conn=%u len=%u ", (long)ts.tv_sec, ts.tv_nsec / 1000, tag, conn, cb);
    const unsigned char *b = data;
    uint32_t n = cb > 128 ? 128 : cb;
    for (uint32_t i = 0; i < n; i++) fprintf(out, "%02x", b[i]);
    if (cb > n) fprintf(out, "..(%u more)", cb - n);
    fputc('\n', out);
}

static int (*orig_send)(void *, uint32_t, const void *, uint32_t, int, int64_t *);
static int (*orig_sendm)(void *, void **, int, int, int64_t *);
static int (*orig_recv)(void *, uint32_t, void **, int);
static int (*orig_recvp)(void *, uint32_t, void **, int);

static int my_send(void *self, uint32_t conn, const void *data, uint32_t cb, int flags, int64_t *num) {
    dump("SEND", conn, data, cb);
    return orig_send(self, conn, data, cb, flags, num);
}

static int my_sendm(void *self, void **msgs, int n, int flags, int64_t *num) {
    for (int i = 0; i < n; i++) {
        // SteamNetworkingMessage_t: data pointer first, size second.
        void **m = (void **)msgs[i];
        if (m && m[0]) dump("SENDM", 0, m[0], (uint32_t)(uintptr_t)m[1]);
    }
    return orig_sendm(self, msgs, n, flags, num);
}

static int my_recv(void *self, uint32_t conn, void **msgs, int max) {
    int r = orig_recv(self, conn, msgs, max);
    for (int i = 0; i < r; i++) {
        void **m = (void **)msgs[i];
        if (m && m[0]) dump("RECV", conn, m[0], (uint32_t)(uintptr_t)m[1]);
    }
    return r;
}

static int my_recvp(void *self, uint32_t group, void **msgs, int max) {
    int r = orig_recvp(self, group, msgs, max);
    for (int i = 0; i < r; i++) {
        void **m = (void **)msgs[i];
        if (m && m[0]) dump("RECVP", group, m[0], (uint32_t)(uintptr_t)m[1]);
    }
    return r;
}

static void *api_handle(void) {
    static void *h;
    if (!h) h = dlopen("libsteam_api.so", RTLD_NOW | RTLD_NOLOAD);
    if (!h) h = dlopen("libsteam_api.so", RTLD_NOW);
    return h;
}

static void patch(void *obj) {
    ensure();
    if (!obj || !out) return;
    void **vt = *(void ***)obj;
    long pg = sysconf(_SC_PAGESIZE);
    unsigned long lo = (unsigned long)vt & ~(unsigned long)(pg - 1);
    unsigned long hi = ((unsigned long)vt + 28 * sizeof(void *) + pg - 1) & ~(unsigned long)(pg - 1);
    if (mprotect((void *)lo, hi - lo, PROT_READ | PROT_WRITE) != 0) {
        fprintf(out, "patch: mprotect failed\n");
        return;
    }
    orig_send = (void *)vt[IDX_SEND];
    orig_sendm = (void *)vt[IDX_SENDM];
    orig_recv = (void *)vt[IDX_RECV];
    orig_recvp = (void *)vt[IDX_RECVP];
    vt[IDX_SEND] = (void *)my_send;
    vt[IDX_SENDM] = (void *)my_sendm;
    vt[IDX_RECV] = (void *)my_recv;
    vt[IDX_RECVP] = (void *)my_recvp;
    mprotect((void *)lo, hi - lo, PROT_READ);
    fprintf(out, "patched vtable=%p\n", (void *)vt);
}

typedef bool (*init_fn)(void);

bool SteamAPI_Init(void) {
    static init_fn real;
    if (!real) real = (init_fn)dlsym(RTLD_NEXT, "SteamAPI_Init");
    bool ok = real ? real() : false;
    if (!ok) return ok;

    void *api = api_handle();
    typedef void *(*sockets_fn)(void);
    static sockets_fn sockets;
    if (!sockets) sockets = (sockets_fn)dlsym(api, "SteamAPI_SteamNetworkingSockets_SteamAPI_v012");
    if (sockets) patch(sockets());
    return ok;
}