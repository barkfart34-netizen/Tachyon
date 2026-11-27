//! Create Aeronautics (`aeronautics`, `simulated`, `offroad`) Horizon native runtime for Tachyon.

pub mod net;
mod render;
mod server;
pub mod solid;

use std::sync::{Arc, LazyLock, Mutex};

use aeronautics_physics::PilotInput;
use mod_api::{
    ClientHost, ClientTickPostEvent, Flow, Hooks, HostSide, IPayloadContext, IPayloadHandler,
    MovementInputEvent, PayloadBytes, PayloadContextError, PayloadDirection, PayloadHandleError,
    PayloadId, RegisterCommandsEvent, Registrar, RegistrationContext, RenderGuiPostEvent,
    RightClickBlockEvent, RightClickItemEvent, RunModCommandEvent, ServerHost, ServerStoppedEvent,
    ServerTickEvent, SubmitCustomGeometryEvent,
};

pub const ID: &str = "tachyon_aeronautics";
pub const VERSION: &str = "0.1.0";

/// Static registration record matching `tachyon/content/tachyon_aeronautics/horizon-mod.toml`.
pub const BUILT: mod_api::__private::native::BuiltMod = mod_api::__private::native::BuiltMod {
    id: ID,
    version: VERSION,
    side: mod_api::Side::Both,
};

static LATEST_INPUT: LazyLock<Mutex<PilotInput>> = LazyLock::new(Mutex::default);

struct NetHandler;

impl IPayloadHandler for NetHandler {
    fn handle(
        &self,
        payload: PayloadBytes,
        context: &mut dyn IPayloadContext,
    ) -> Result<(), PayloadHandleError> {
        let failed = |e: PayloadContextError| PayloadHandleError::Failed(format!("{e:?}"));
        match context.flow() {
            PayloadDirection::ToServer => {
                let player = context.player().map_err(failed)?.uuid();
                let host = context.server_host().map_err(failed)?;
                server::on_client_packet(host, player, &payload.0);
            }
            _ => {
                if let Some(msg) = net::ServerMsg::decode(&payload.0) {
                    match msg {
                        net::ServerMsg::Snapshot(snap) => solid::apply_snapshot(snap),
                        net::ServerMsg::Remove(id) => solid::remove_vessel(id),
                        net::ServerMsg::SetLocalPilot(id) => solid::set_local_pilot_vessel(id),
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        let mut handlers = r.payload_handlers();
        let mut view = handlers.registrar("1");
        let handler = Arc::new(NetHandler);
        view.play_bidirectional(
            PayloadId(net::PAYLOAD_ID.to_owned()),
            handler.clone(),
            Some(handler),
        );
    }

    fn server_stopped(&self, _host: &mut dyn ServerHost, _event: &mut ServerStoppedEvent) {
        server::clear();
        solid::clear();
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        server::server_tick(host, event.phase);
    }

    fn right_click_block(
        &self,
        host: HostSide<'_>,
        event: &mut RightClickBlockEvent<'_>,
    ) -> Flow {
        match host {
            HostSide::Server(server_host) => server::right_click_block(server_host, event),
            HostSide::Client(_) => Flow::Continue,
        }
    }

    fn right_click_item(
        &self,
        host: HostSide<'_>,
        event: &mut RightClickItemEvent<'_>,
    ) -> Flow {
        match host {
            HostSide::Server(server_host) => server::right_click_item(server_host, event),
            HostSide::Client(_) => Flow::Continue,
        }
    }

    fn register_commands(
        &self,
        _host: &mut dyn ServerHost,
        event: &mut RegisterCommandsEvent<'_>,
    ) {
        server::register_commands(event.commands);
    }

    fn run_mod_command(
        &self,
        host: &mut dyn ServerHost,
        event: &mut RunModCommandEvent<'_>,
    ) -> Flow {
        server::run_mod_command(host, event.source, event.command)
    }

    fn movement_input(&self, host: &dyn ClientHost, event: &mut MovementInputEvent) {
        let Some(_vid) = solid::local_pilot_vessel_id() else {
            return;
        };
        let vertical = if event.jump {
            1.0
        } else if event.sneak {
            -1.0
        } else {
            0.0
        };
        let brake = host.is_key_down("key.keyboard.b");
        let input = PilotInput {
            throttle: event.forward.clamp(-1.0, 1.0),
            pitch: 0.0,
            yaw: event.strafe.clamp(-1.0, 1.0),
            roll: 0.0,
            vertical,
            brake,
        };
        if let Ok(mut guard) = LATEST_INPUT.lock() {
            *guard = input;
        }
        // Keep player movement from fighting vessel helm seat while piloting.
        event.forward = 0.0;
        event.strafe = 0.0;
        event.jump = false;
        event.sneak = false;
    }

    fn client_tick_post(&self, host: &mut dyn ClientHost, _event: &mut ClientTickPostEvent) {
        let Some(vid) = solid::local_pilot_vessel_id() else { return };
        let input = LATEST_INPUT
            .lock()
            .map(|g| *g)
            .unwrap_or_default();
        let pkt = net::PilotPacket {
            vessel_id: vid,
            input,
            action: net::ACTION_CONTROL,
        };
        let _ = host.send_payload_to_server(
            &PayloadId(net::PAYLOAD_ID.to_owned()),
            PayloadBytes(pkt.encode()),
        );
    }

    fn submit_custom_geometry(
        &self,
        host: &dyn ClientHost,
        event: &mut SubmitCustomGeometryEvent<'_>,
    ) {
        render::submit_custom_geometry(host, event);
    }

    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        render::render_gui_post(host, event);
    }
}
