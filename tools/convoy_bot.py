#!/usr/bin/env python3
"""Headless ETS2 Convoy client for two-client tests.

Speaks the same handshake as the stock game against an OpenConvoy server:
challenge -> auth ticket -> mod list -> wait for the connect ack -> announce a
`player_info` entity -> confirm ack -> 10 Hz telemetry.

The bot attaches to the Steam client already running on this machine, so it
uses the same SteamID as the game (a separate connection, which the server keys
independently).

    STEAM_API_PATH=/path/to/libsteam_api.so \
        python3 convoy_bot.py --server <steamid> --name GhostTruck --duration 60
"""
import argparse
import ctypes
import math
import os
import struct
import sys
import time

LIB_PATH = os.environ.get(
    "STEAM_API_PATH", "/home/lumi/ets2-dedicated/bin/linux_x64/libsteam_api.so"
)
os.environ.setdefault("SteamAppId", "227300")

if not os.path.exists(LIB_PATH):
    sys.exit(f"missing libsteam_api.so at {LIB_PATH}")

lib = ctypes.CDLL(LIB_PATH)


class Identity(ctypes.Structure):
    _fields_ = [("raw", ctypes.c_uint8 * 136)]


lib.SteamAPI_Init.restype = ctypes.c_bool
lib.SteamAPI_Shutdown.restype = None
lib.SteamAPI_RunCallbacks.restype = None
lib.SteamAPI_SteamNetworkingSockets_SteamAPI_v012.restype = ctypes.c_void_p
lib.SteamAPI_SteamUser_v023.restype = ctypes.c_void_p
lib.SteamAPI_SteamNetworkingIdentity_Clear.argtypes = [ctypes.POINTER(Identity)]
lib.SteamAPI_SteamNetworkingIdentity_SetSteamID64.argtypes = [ctypes.POINTER(Identity), ctypes.c_uint64]
lib.SteamAPI_ISteamNetworkingSockets_ConnectP2P.argtypes = [
    ctypes.c_void_p, ctypes.POINTER(Identity), ctypes.c_int, ctypes.c_int, ctypes.c_void_p
]
lib.SteamAPI_ISteamNetworkingSockets_ConnectP2P.restype = ctypes.c_uint32
lib.SteamAPI_ISteamNetworkingSockets_SendMessageToConnection.argtypes = [
    ctypes.c_void_p, ctypes.c_uint32, ctypes.c_char_p, ctypes.c_uint32, ctypes.c_int, ctypes.c_void_p
]
lib.SteamAPI_ISteamNetworkingSockets_SendMessageToConnection.restype = ctypes.c_int
lib.SteamAPI_ISteamNetworkingSockets_ReceiveMessagesOnConnection.argtypes = [
    ctypes.c_void_p, ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p), ctypes.c_int
]
lib.SteamAPI_ISteamNetworkingSockets_ReceiveMessagesOnConnection.restype = ctypes.c_int
lib.SteamAPI_ISteamNetworkingSockets_CloseConnection.argtypes = [
    ctypes.c_void_p, ctypes.c_uint32, ctypes.c_int, ctypes.c_char_p, ctypes.c_bool
]
lib.SteamAPI_ISteamUser_GetAuthSessionTicket.argtypes = [
    ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.POINTER(ctypes.c_uint32)
]
lib.SteamAPI_ISteamUser_GetAuthSessionTicket.restype = ctypes.c_uint32

RELIABLE = 8
UNRELIABLE = 0
PLAYER_INFO_TOKEN = bytes.fromhex("a245545f34633702")  # base-38 "player_info"


def encode_quat(yaw_deg):
    y = math.radians(yaw_deg) * 0.5
    return int((0.5 + y / (2 * math.pi)) * 0xFFFFFFFF) & 0xFFFFFFFF


class Bot:
    def __init__(self, server_steamid, name="GhostTruck", x=55.0, y=13.3, z=-230.0):
        self.server = server_steamid
        self.name = name
        self.x, self.y, self.z = x, y, z
        self.sock = None
        self.user = None
        self.conn = None
        self.cid = None
        self.sent_ticket = False
        self.sent_mods = False
        self.spawned = False
        self.last_tick = 0.0
        self.buf = (ctypes.c_void_p * 32)()

    def _send(self, data, flags=RELIABLE):
        lib.SteamAPI_ISteamNetworkingSockets_SendMessageToConnection(
            self.sock, self.conn, data, len(data), flags, None
        )

    def start(self):
        if not lib.SteamAPI_Init():
            print("[-] SteamAPI_Init failed; is Steam running?")
            return False
        self.sock = lib.SteamAPI_SteamNetworkingSockets_SteamAPI_v012()
        self.user = lib.SteamAPI_SteamUser_v023()
        ident = Identity()
        lib.SteamAPI_SteamNetworkingIdentity_Clear(ctypes.byref(ident))
        lib.SteamAPI_SteamNetworkingIdentity_SetSteamID64(ctypes.byref(ident), self.server)
        self.conn = lib.SteamAPI_ISteamNetworkingSockets_ConnectP2P(
            self.sock, ctypes.byref(ident), 100, 0, None
        )
        print(f"[+] connection handle {self.conn}")
        return True

    def _ticket(self):
        buf = (ctypes.c_uint8 * 1024)()
        size = ctypes.c_uint32(0)
        lib.SteamAPI_ISteamUser_GetAuthSessionTicket(
            self.user, ctypes.byref(buf), 1024, ctypes.byref(size)
        )
        return bytes(buf)[: size.value]

    # Captured from a real join (cid 0x4a); the two id fields are substituted.
    MOD_LIST = bytes.fromhex(
        "00000100004a004a00020000000000000000bd090000b46dffff0566c7300000000000"
        "000000007f0000364533343042394346464233374139383943413534344536424237"
        "3830413243373839303144334642333337333837363835313141333036313741464130"
        "314400c6c14336b4789dede1770301000030010f37f6f6ea"
    )

    def _mod_list(self):
        frame = bytearray(self.MOD_LIST)
        frame[5:7] = struct.pack("<H", self.cid)
        frame[7:9] = struct.pack("<H", self.cid)
        return bytes(frame)

    # 61-byte player_info create captured from a real client (cid 0x4a).
    SPAWN = bytearray.fromhex(
        "000002f7ff4a0000a245545f346337024a0001000000ffffffff000004f7ff4a004a00"
        "0100000000800080000084c21b45504c12c7044c756d69000900"
    )

    def _player_spawn(self):
        frame = bytearray(self.SPAWN)
        for off in (5, 16, 31, 33):
            frame[off:off + 2] = struct.pack("<H", self.cid)
        name = self.name.encode()[:4].ljust(4, b"\0")
        frame[53] = 4
        frame[54:58] = name
        return bytes(frame)

    def _ack(self):
        cid = self.cid
        return b"\x00\x00\x01\x00\x00" + struct.pack("<HH", cid, cid) + b"\x06" + b"\x00" * 8

    def _telemetry(self):
        cid = self.cid
        hdr = bytes.fromhex("000004f6ff") + struct.pack("<HH", cid, cid) + bytes.fromhex(
            "2c00000031ff69fec701fa0011005a883d000000cef82cf7f060010001"
        )
        return (
            hdr
            + struct.pack("<3f", self.x, self.y, self.z)
            + bytes.fromhex("3d0c304090c184c299ff35ff")
            + struct.pack("<I", encode_quat((time.time() * 30) % 360))
        )

    def poll(self):
        while True:
            n = lib.SteamAPI_ISteamNetworkingSockets_ReceiveMessagesOnConnection(
                self.sock, self.conn, self.buf, 32
            )
            if n <= 0:
                return
            for i in range(n):
                ptr = self.buf[i]
                size = ctypes.c_int32.from_address(ptr + 8).value
                data = ctypes.c_void_p.from_address(ptr).value
                raw = bytes((ctypes.c_uint8 * size).from_address(data))
                self.handle(raw)

    def handle(self, raw):
        if len(raw) >= 5 and raw[2] == 0x0B and self.cid is None:
            self.cid = struct.unpack_from("<H", raw, 3)[0]
            print(f"[*] client id {self.cid}")

        if self.cid is None:
            return

        if not self.sent_ticket and raw[2] == 0x0B:
            ticket = self._ticket()
            self._send(b"\x00\x00\x0b" + struct.pack("<H", self.cid) + b"\x00\x00" + ticket)
            print(f"[+] ticket sent ({len(ticket)} bytes)")
            self.sent_ticket = True

        if self.sent_ticket and not self.sent_mods:
            self._send(self._mod_list())
            print("[+] mod list sent")
            self.sent_mods = True

        # Connect ack (type 0x01, subtype 0x05) means the world is ready.
        if self.sent_mods and not self.spawned and len(raw) == 18 and raw[2] == 0x01 and raw[9] == 0x05:
            self._send(self._player_spawn())
            self._send(self._ack())
            print(f"[+] spawned as {self.name} (player_info, net 1)")
            self.spawned = True

    def run(self, duration):
        t0 = time.time()
        while time.time() - t0 < duration:
            lib.SteamAPI_RunCallbacks()
            self.poll()
            now = time.time()
            if self.spawned and now - self.last_tick >= 0.1:
                self._send(self._telemetry(), UNRELIABLE)
                self.last_tick = now
            time.sleep(0.01)

    def stop(self):
        if self.sock and self.conn:
            lib.SteamAPI_ISteamNetworkingSockets_CloseConnection(
                self.sock, self.conn, 0, b"bot quit", False
            )
        lib.SteamAPI_Shutdown()


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True, help="server SteamID64")
    ap.add_argument("--name", default="GhostTruck")
    ap.add_argument("--duration", type=float, default=60.0)
    args = ap.parse_args()

    bot = Bot(int(args.server), args.name)
    if bot.start():
        try:
            bot.run(args.duration)
        except KeyboardInterrupt:
            pass
        finally:
            bot.stop()