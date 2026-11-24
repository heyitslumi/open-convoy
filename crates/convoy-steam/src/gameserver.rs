use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use libloading::{Library, Symbol};

use crate::sockets::SteamSockets;
use crate::SteamError;

pub const DEFAULT_STEAM_API_PATH: &str =
    "/home/lumi/ets2-dedicated/bin/linux_x64/libsteam_api.so";

type FnGameServerInit = unsafe extern "C" fn(
    un_ip: u32,
    us_game_port: u16,
    us_query_port: u16,
    us_spectator_port: u16,
    e_server_mode: i32,
    pch_version: *const c_char,
) -> bool;

type FnGameServerShutdown = unsafe extern "C" fn();
type FnGameServerRunCallbacks = unsafe extern "C" fn();
type FnGameServerGetHSteamUser = unsafe extern "C" fn() -> u32;
type FnFindOrCreateGameServerInterface =
    unsafe extern "C" fn(h_user: u32, ver: *const c_char) -> *mut c_void;

type FnBeginAuthSession = unsafe extern "C" fn(
    self_ptr: *mut c_void,
    p_auth_ticket: *const c_void,
    cb_auth_ticket: i32,
    steam_id: u64,
) -> i32;

type FnEndAuthSession =
    unsafe extern "C" fn(self_ptr: *mut c_void, steam_id: u64);

type FnLogOn = unsafe extern "C" fn(self_ptr: *mut c_void, psz_token: *const c_char);
type FnLogOnAnonymous = unsafe extern "C" fn(self_ptr: *mut c_void);

pub(crate) struct SteamApiSymbols {
    _lib: Library,
    pub(crate) init_fn: FnGameServerInit,
    pub(crate) shutdown_fn: FnGameServerShutdown,
    pub(crate) run_callbacks_fn: FnGameServerRunCallbacks,
    pub(crate) run_socket_callbacks_fn: unsafe extern "C" fn(*mut c_void),
    pub(crate) get_h_steam_user_fn: FnGameServerGetHSteamUser,
    pub(crate) find_or_create_iface_fn: FnFindOrCreateGameServerInterface,
    pub(crate) begin_auth_session_fn: FnBeginAuthSession,
    pub(crate) end_auth_session_fn: FnEndAuthSession,
    pub(crate) log_on_fn: FnLogOn,
    pub(crate) log_on_anonymous_fn: FnLogOnAnonymous,
    pub(crate) get_steam_id_fn: unsafe extern "C" fn(self_ptr: *mut c_void) -> u64,
    pub(crate) set_product_fn: unsafe extern "C" fn(self_ptr: *mut c_void, product: *const c_char),
    pub(crate) set_game_description_fn: unsafe extern "C" fn(self_ptr: *mut c_void, desc: *const c_char),
    pub(crate) set_mod_dir_fn: unsafe extern "C" fn(self_ptr: *mut c_void, mod_dir: *const c_char),
    pub(crate) set_dedicated_server_fn: unsafe extern "C" fn(self_ptr: *mut c_void, dedicated: bool),
    pub(crate) set_max_player_count_fn: unsafe extern "C" fn(self_ptr: *mut c_void, max_players: i32),
    pub(crate) set_server_name_fn: unsafe extern "C" fn(self_ptr: *mut c_void, name: *const c_char),
    pub(crate) set_map_name_fn: unsafe extern "C" fn(self_ptr: *mut c_void, map: *const c_char),
    pub(crate) set_game_tags_fn: unsafe extern "C" fn(self_ptr: *mut c_void, tags: *const c_char),
    set_key_value_fn: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
    pub(crate) set_advertise_server_active_fn: unsafe extern "C" fn(self_ptr: *mut c_void, active: bool),
    // Sockets symbols
    pub(crate) create_listen_socket_p2p_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void, vport: i32, n_options: i32, p_options: *const c_void) -> u32,
    pub(crate) create_listen_socket_ip_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void, p_address: *const c_void, n_options: i32, p_options: *const c_void) -> u32,
    pub(crate) create_poll_group_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void) -> u32,
    pub(crate) set_connection_poll_group_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void, h_conn: u32, h_poll_group: u32) -> bool,
    pub(crate) receive_messages_on_poll_group_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void, h_poll_group: u32, pp_out_messages: *mut *mut c_void, n_max_messages: i32) -> i32,
    pub(crate) accept_connection_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void, h_conn: u32) -> i32,
    pub(crate) set_status_callback_fn:
        Option<unsafe extern "C" fn(utils_ptr: *mut c_void, cb: unsafe extern "C" fn(*mut c_void)) -> bool>,
    pub(crate) send_message_to_connection_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void, h_conn: u32, p_data: *const c_void, cb_data: u32, n_send_flags: i32, p_out_message_number: *mut i64) -> i32,
    pub(crate) close_connection_fn:
        unsafe extern "C" fn(self_ptr: *mut c_void, h_conn: u32, n_reason: i32, psz_debug: *const c_char, b_enable_linger: bool) -> bool,
}

impl SteamApiSymbols {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, SteamError> {
        unsafe {
            let lib = Library::new(path.as_ref())
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;

            let shutdown_sym: Symbol<FnGameServerShutdown> = lib
                .get(b"SteamGameServer_Shutdown\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let shutdown_fn = *shutdown_sym;

            let run_callbacks_sym: Symbol<FnGameServerRunCallbacks> = lib
                .get(b"SteamGameServer_RunCallbacks\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let run_callbacks_fn = *run_callbacks_sym;
            let run_socket_callbacks_fn = *lib.get(b"SteamAPI_ISteamNetworkingSockets_RunCallbacks\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;

            let get_h_steam_user_sym: Symbol<FnGameServerGetHSteamUser> = lib
                .get(b"SteamGameServer_GetHSteamUser\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let get_h_steam_user_fn = *get_h_steam_user_sym;

            let find_or_create_iface_sym: Symbol<FnFindOrCreateGameServerInterface> = lib
                .get(b"SteamInternal_FindOrCreateGameServerInterface\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let find_or_create_iface_fn = *find_or_create_iface_sym;

            let begin_auth_sym: Symbol<FnBeginAuthSession> = lib
                .get(b"SteamAPI_ISteamGameServer_BeginAuthSession\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let begin_auth_session_fn = *begin_auth_sym;

            let end_auth_sym: Symbol<FnEndAuthSession> = lib
                .get(b"SteamAPI_ISteamGameServer_EndAuthSession\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let end_auth_session_fn = *end_auth_sym;

            let log_on_sym: Symbol<FnLogOn> = lib
                .get(b"SteamAPI_ISteamGameServer_LogOn\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let log_on_fn = *log_on_sym;

            let log_on_anon_sym: Symbol<FnLogOnAnonymous> = lib
                .get(b"SteamAPI_ISteamGameServer_LogOnAnonymous\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let log_on_anonymous_fn = *log_on_anon_sym;

            let get_steam_id_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void) -> u64> = lib
                .get(b"SteamAPI_ISteamGameServer_GetSteamID\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let get_steam_id_fn = *get_steam_id_sym;

            let set_product_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, product: *const c_char)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetProduct\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_product_fn = *set_product_sym;

            let set_desc_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, desc: *const c_char)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetGameDescription\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_game_description_fn = *set_desc_sym;

            let set_mod_dir_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, mod_dir: *const c_char)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetModDir\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_mod_dir_fn = *set_mod_dir_sym;

            let set_dedicated_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, dedicated: bool)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetDedicatedServer\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_dedicated_server_fn = *set_dedicated_sym;

            let set_max_players_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, max_players: i32)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetMaxPlayerCount\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_max_player_count_fn = *set_max_players_sym;

            let set_server_name_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, name: *const c_char)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetServerName\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_server_name_fn = *set_server_name_sym;

            let set_map_name_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, map: *const c_char)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetMapName\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_map_name_fn = *set_map_name_sym;

            let set_game_tags_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, tags: *const c_char)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetGameTags\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_game_tags_fn = *set_game_tags_sym;
            let set_key_value_fn = *lib
                .get(b"SteamAPI_ISteamGameServer_SetKeyValue\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;

            let set_adv_sym: Symbol<unsafe extern "C" fn(self_ptr: *mut c_void, active: bool)> = lib
                .get(b"SteamAPI_ISteamGameServer_SetAdvertiseServerActive\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_advertise_server_active_fn = *set_adv_sym;

            let create_listen_socket_p2p_sym: Symbol<
                unsafe extern "C" fn(
                    self_ptr: *mut c_void,
                    vport: i32,
                    n_options: i32,
                    p_options: *const c_void,
                ) -> u32,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_CreateListenSocketP2P\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let create_listen_socket_p2p_fn = *create_listen_socket_p2p_sym;

            let create_listen_socket_ip_sym: Symbol<
                unsafe extern "C" fn(
                    self_ptr: *mut c_void,
                    p_address: *const c_void,
                    n_options: i32,
                    p_options: *const c_void,
                ) -> u32,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_CreateListenSocketIP\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let create_listen_socket_ip_fn = *create_listen_socket_ip_sym;

            let create_poll_group_sym: Symbol<
                unsafe extern "C" fn(self_ptr: *mut c_void) -> u32,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_CreatePollGroup\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let create_poll_group_fn = *create_poll_group_sym;

            let set_connection_poll_group_sym: Symbol<
                unsafe extern "C" fn(self_ptr: *mut c_void, h_conn: u32, h_poll_group: u32) -> bool,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_SetConnectionPollGroup\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let set_connection_poll_group_fn = *set_connection_poll_group_sym;

            let receive_messages_on_poll_group_sym: Symbol<
                unsafe extern "C" fn(
                    self_ptr: *mut c_void,
                    h_poll_group: u32,
                    pp_out_messages: *mut *mut c_void,
                    n_max_messages: i32,
                ) -> i32,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_ReceiveMessagesOnPollGroup\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let receive_messages_on_poll_group_fn = *receive_messages_on_poll_group_sym;

            let accept_connection_sym: Symbol<
                unsafe extern "C" fn(self_ptr: *mut c_void, h_conn: u32) -> i32,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_AcceptConnection\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let accept_connection_fn = *accept_connection_sym;

            let set_status_callback_fn = lib
                .get::<unsafe extern "C" fn(utils_ptr: *mut c_void, cb: unsafe extern "C" fn(*mut c_void)) -> bool>(
                    b"SteamAPI_ISteamNetworkingUtils_SetGlobalCallback_SteamNetConnectionStatusChanged\0",
                )
                .ok()
                .map(|s| *s);

            let send_message_to_connection_sym: Symbol<
                unsafe extern "C" fn(
                    self_ptr: *mut c_void,
                    h_conn: u32,
                    p_data: *const c_void,
                    cb_data: u32,
                    n_send_flags: i32,
                    p_out_message_number: *mut i64,
                ) -> i32,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_SendMessageToConnection\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let send_message_to_connection_fn = *send_message_to_connection_sym;

            let close_connection_sym: Symbol<
                unsafe extern "C" fn(
                    self_ptr: *mut c_void,
                    h_conn: u32,
                    n_reason: i32,
                    psz_debug: *const c_char,
                    b_enable_linger: bool,
                ) -> bool,
            > = lib
                .get(b"SteamAPI_ISteamNetworkingSockets_CloseConnection\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let close_connection_fn = *close_connection_sym;

            // Init function resolution
            let init_sym: Symbol<FnGameServerInit> = lib
                .get(b"SteamInternal_GameServer_Init\0")
                .map_err(|e| SteamError::LibraryLoadError(e.to_string()))?;
            let init_fn = *init_sym;

            Ok(SteamApiSymbols {
                _lib: lib,
                init_fn,
                shutdown_fn,
                run_callbacks_fn,
                run_socket_callbacks_fn,
                get_h_steam_user_fn,
                find_or_create_iface_fn,
                begin_auth_session_fn,
                end_auth_session_fn,
                log_on_fn,
                log_on_anonymous_fn,
                get_steam_id_fn,
                set_product_fn,
                set_game_description_fn,
                set_mod_dir_fn,
                set_dedicated_server_fn,
                set_max_player_count_fn,
                set_server_name_fn,
                set_map_name_fn,
                set_game_tags_fn,
                set_key_value_fn,
                set_advertise_server_active_fn,
                create_listen_socket_p2p_fn,
                create_listen_socket_ip_fn,
                create_poll_group_fn,
                set_connection_poll_group_fn,
                receive_messages_on_poll_group_fn,
                accept_connection_fn,
                set_status_callback_fn,
                send_message_to_connection_fn,
                close_connection_fn,
            })
        }
    }
}

pub struct SteamServer {
    pub(crate) symbols: Arc<SteamApiSymbols>,
    pub(crate) game_server_iface: *mut c_void,
    sockets: SteamSockets,
}

// SteamServer is safe to Send across threads
unsafe impl Send for SteamServer {}
unsafe impl Sync for SteamServer {}

impl SteamServer {
    pub fn init(game_port: u16, query_port: u16, server_name: &str, server_logon_token: Option<&str>) -> Result<Self, SteamError> {
        let server_name_c = CString::new(server_name).map_err(|_| SteamError::SocketError("invalid server name".into()))?;
        let lib_path = std::env::var("STEAM_API_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_STEAM_API_PATH));

        let symbols = Arc::new(SteamApiSymbols::load(lib_path)?);
        let version_cstr = CString::new("1.0.0.1").unwrap();

        let ok = unsafe {
            // SteamInternal_GameServer_Init(unIP=0, usGamePort=0, usQueryPort=27015, usSpectatorPort=27016, eServerMode=2, pchVersion="1.0.0.1")
            (symbols.init_fn)(0, 0, game_port, query_port, 2, version_cstr.as_ptr())
        };

        if !ok {
            return Err(SteamError::InitFailed);
        }

        let h_user = unsafe { (symbols.get_h_steam_user_fn)() };

        let gs_ver = CString::new("SteamGameServer015").unwrap();
        let game_server_iface = unsafe {
            (symbols.find_or_create_iface_fn)(h_user, gs_ver.as_ptr())
        };

        let sockets_ver = CString::new("SteamNetworkingSockets012").unwrap();
        let sockets_iface = unsafe {
            (symbols.find_or_create_iface_fn)(h_user, sockets_ver.as_ptr())
        };

        let utils_ver = CString::new("SteamNetworkingUtils004").unwrap();
        let utils_iface = unsafe {
            (symbols.find_or_create_iface_fn)(h_user, utils_ver.as_ptr())
        };

        if game_server_iface.is_null() || sockets_iface.is_null() {
            unsafe { (symbols.shutdown_fn)() };
            return Err(SteamError::InitFailed);
        }

        unsafe {
            let product = CString::new("eut2").unwrap();
            let desc = CString::new("Euro Truck Simulator 2").unwrap();
            let mod_dir = CString::new("Dzf29urgpnA=").unwrap();
            let map_name_c = CString::new("/map/europe.mbd").unwrap();
            let tags_c = CString::new("ZABlAEsAAAA=").unwrap();

            (symbols.set_product_fn)(game_server_iface, product.as_ptr());
            (symbols.set_game_description_fn)(game_server_iface, desc.as_ptr());
            (symbols.set_mod_dir_fn)(game_server_iface, mod_dir.as_ptr());
            (symbols.set_dedicated_server_fn)(game_server_iface, false);
            (symbols.set_max_player_count_fn)(game_server_iface, 8);
            (symbols.set_server_name_fn)(game_server_iface, server_name_c.as_ptr());
            (symbols.set_map_name_fn)(game_server_iface, map_name_c.as_ptr());
            (symbols.set_game_tags_fn)(game_server_iface, tags_c.as_ptr());
            // The stock ETS2 server returns this rule in its A2S_RULES reply.
            (symbols.set_key_value_fn)(game_server_iface, c"D0".as_ptr(), c"0".as_ptr());
        }

        let server = Self {
            symbols: Arc::clone(&symbols),
            game_server_iface,
            sockets: SteamSockets::new(Arc::clone(&symbols), sockets_iface, utils_iface),
        };

        if let Some(token) = server_logon_token {
            server.log_on(token)?;
        } else {
            server.log_on_anonymous();
        }

        unsafe {
            (server.symbols.set_advertise_server_active_fn)(server.game_server_iface, true);
        }

        Ok(server)
    }

    pub fn run_callbacks(&self) {
        unsafe {
            (self.symbols.run_callbacks_fn)();
            self.sockets.run_callbacks();
        }
    }

    pub fn log_on(&self, token: &str) -> Result<(), SteamError> {
        let c_token = CString::new(token).map_err(|_| SteamError::AuthFailed(-1))?;
        unsafe {
            (self.symbols.log_on_fn)(self.game_server_iface, c_token.as_ptr());
        }
        Ok(())
    }

    pub fn log_on_anonymous(&self) {
        unsafe {
            (self.symbols.log_on_anonymous_fn)(self.game_server_iface);
        }
    }

    pub fn steam_id(&self) -> u64 {
        unsafe { (self.symbols.get_steam_id_fn)(self.game_server_iface) }
    }

    pub fn begin_auth_session(&self, ticket: &[u8], steam_id: u64) -> Result<(), SteamError> {
        let res = unsafe {
            (self.symbols.begin_auth_session_fn)(
                self.game_server_iface,
                ticket.as_ptr() as *const c_void,
                ticket.len() as i32,
                steam_id,
            )
        };
        if res == 0 {
            Ok(())
        } else {
            Err(SteamError::AuthFailed(res))
        }
    }

    pub fn end_auth_session(&self, steam_id: u64) {
        unsafe {
            (self.symbols.end_auth_session_fn)(self.game_server_iface, steam_id);
        }
    }

    pub fn sockets(&self) -> &SteamSockets {
        &self.sockets
    }
}

impl Drop for SteamServer {
    fn drop(&mut self) {
        unsafe {
            (self.symbols.shutdown_fn)();
        }
    }
}
