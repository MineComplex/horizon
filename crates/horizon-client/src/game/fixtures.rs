//! The component fixtures (`HORIZON_DISPENSE_FIXTURE`, `HORIZON_SCULK_FIXTURE`)
//! run on `horizon-server`: their own synthetic levels and entity phases on a
//! server Game, without a client.
use super::{Game, Options, PackResources, Resources, UiResources, execution_role::ExecutionRole};
use horizon_client::{Result, audio::Audio, hud::HudAtlas, settings::Settings};
use std::sync::Arc;

pub(super) enum ComponentHarness {
    Dispenser(String),
    Sculk(String),
}

impl Game<'_> {
    pub(super) fn run_component_harness(&self, fixture: ComponentHarness) -> Result<()> {
        let (res, pack, settings) = (self.res.clone(), self.pack.clone(), self.settings.clone());
        let mut options = self.options.clone();
        options.execution_role = ExecutionRole::DedicatedServer;
        options.dedicated = Some(Vec::new());
        let result = std::thread::Builder::new()
            .name("Server thread".into())
            .spawn(move || {
                let run = || -> Result<()> {
                    let mut server = server_game(&res, pack, options, settings)?;
                    match fixture {
                        ComponentHarness::Dispenser(spec) => server.dispense_fixture(&spec),
                        ComponentHarness::Sculk(spec) => server.sculk_fixture(&spec),
                    }
                };
                run().map_err(|e| e.to_string())
            })?
            .join()
            .map_err(|_| "the component harness server panicked")?;
        result.map_err(Into::into)
    }
}

/// A server Game: it draws nothing, only the font, which books and chat
/// measure with.
fn server_game(
    res: &Arc<Resources>,
    pack: PackResources,
    options: Options,
    settings: Settings,
) -> Result<Game<'_>> {
    let atlas = HudAtlas::fonts(&options.resources)?;
    let gui = UiResources {
        atlas,
        panorama: Arc::new(horizon_client::gfx::panorama::Faces {
            size: 1,
            pixels: [0u8; 4].repeat(6),
        }),
    };
    let audio = Audio::silent(&options.resources);
    Game::new(res, pack, gui, options, settings, audio, None)
}
