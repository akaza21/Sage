impl std::fmt::Debug for super::AcpService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpService")
            .field("database", &self.database)
            .field("scheduler", &self.scheduler)
            .field("journal", &self.journal)
            .field("sessions", &self.sessions)
            .field("runtime_active", &self.runtime.is_active())
            .field(
                "codex_admitted",
                &self.registry.codex_admission().is_available(),
            )
            .field("notification_active", &self.notifier.is_some())
            .finish_non_exhaustive()
    }
}
