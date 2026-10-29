//! `/random` (RandomCommand) and `/teammsg` (TeamMsgCommand).
use horizon_client::{clock::SprintReport, text::Text};

impl crate::game::Sim<'_> {
    /// The report `/tick sprint` prints when a sprint ends.
    pub(in crate::game) fn sprint_report(&mut self, report: &SprintReport) {
        self.feedback(Text::translate(
            "commands.tick.sprint.report",
            &[
                Text::literal(report.ticks_per_second.to_string()),
                Text::literal(report.milliseconds_per_tick.clone()),
            ],
        ));
    }
}
