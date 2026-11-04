use std::time::Duration;

pub(crate) fn should_release_idle_session(
    has_active_prompt: bool,
    idle_duration: Duration,
    idle_timeout: Duration,
) -> bool {
    !has_active_prompt && idle_duration >= idle_timeout
}
