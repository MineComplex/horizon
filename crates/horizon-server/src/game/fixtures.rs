//! The component fixtures (`HORIZON_DISPENSE_FIXTURE`, `HORIZON_SCULK_FIXTURE`)
//! run on `horizon-server`: their own synthetic levels and entity phases on a
//! server Game, without a client.
use super::{Options, PackResources, Resources, Settings, execution_role::ExecutionRole};
use crate::hc::Result;
use std::sync::Arc;

pub(super) enum ComponentHarness {
    Dispenser(String),
    Sculk(String),
}

impl crate::game::Sim<'_> {
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

/// A server game for the fixture's own thread.
fn server_game(
    res: &Arc<Resources>,
    pack: PackResources,
    options: Options,
    settings: Settings,
) -> Result<crate::game::Sim<'_>> {
    crate::game::Sim::new(res, pack, options, settings, None)
}
