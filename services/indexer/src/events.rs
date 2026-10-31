//! Ownership-aware extraction of Tidebook event payloads from execution logs.

use solana_pubkey::Pubkey;

const PROGRAM_PREFIX: &str = "Program ";
const INVOKE_MARKER: &str = " invoke [";
const SUCCESS_SUFFIX: &str = " success";
const FAILED_MARKER: &str = " failed: ";
const PROGRAM_DATA_PREFIX: &str = "Program data: ";

/// Raw program-data payload proven to have been emitted by Tidebook itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEventLog {
    /// Original position in the transaction's complete log list.
    pub log_index: usize,

    /// Base64 payload exactly as emitted by `sol_log_data`.
    pub payload_base64: String,
}

/// Extracts data logs emitted directly by Tidebook.
///
/// Solana transactions can invoke multiple nested programs. We therefore track
/// the active invocation stack and accept `Program data:` only when Tidebook is
/// at the top of that stack. This prevents a nested CPI program from emitting
/// bytes that the Tidebook decoder would otherwise misclassify as protocol
/// state. The function preserves original log indexes for stable event keys.
pub fn extract_program_data(logs: &[String], program_id: &Pubkey) -> Vec<RawEventLog> {
    let target_program = program_id.to_string();
    let mut invocation_stack: Vec<&str> = Vec::new();
    let mut events = Vec::new();

    for (log_index, log) in logs.iter().enumerate() {
        if let Some(invoked_program) = parse_invocation(log) {
            invocation_stack.push(invoked_program);
            continue;
        }

        if let Some(payload) = log.strip_prefix(PROGRAM_DATA_PREFIX) {
            let emitted_by_target = invocation_stack
                .last()
                .is_some_and(|active_program| *active_program == target_program.as_str());

            if emitted_by_target {
                events.push(RawEventLog {
                    log_index,
                    payload_base64: payload.to_owned(),
                });
            }

            continue;
        }

        if let Some(completed_program) = parse_completion(log) {
            // Only pop a matching frame. A malformed or incomplete log stream
            // must not silently corrupt our view of the invocation stack.
            let completes_active_program = invocation_stack
                .last()
                .is_some_and(|active_program| *active_program == completed_program);

            if completes_active_program {
                invocation_stack.pop();
            }
        }
    }

    events
}

fn parse_invocation(log: &str) -> Option<&str> {
    let remainder = log.strip_prefix(PROGRAM_PREFIX)?;
    let (program_id, _) = remainder.split_once(INVOKE_MARKER)?;

    Some(program_id)
}

fn parse_completion(log: &str) -> Option<&str> {
    let remainder = log.strip_prefix(PROGRAM_PREFIX)?;

    if let Some(program_id) = remainder.strip_suffix(SUCCESS_SUFFIX) {
        return Some(program_id);
    }

    remainder
        .split_once(FAILED_MARKER)
        .map(|(program_id, _)| program_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logs(lines: &[String]) -> Vec<String> {
        lines.to_vec()
    }

    #[test]
    fn extracts_target_events_in_original_log_order() {
        let target = Pubkey::new_from_array([1; 32]);
        let lines = logs(&[
            format!("Program {target} invoke [1]"),
            "Program log: before".to_owned(),
            "Program data: first".to_owned(),
            "Program data: second".to_owned(),
            format!("Program {target} success"),
        ]);

        let events = extract_program_data(&lines, &target);

        assert_eq!(
            events,
            vec![
                RawEventLog {
                    log_index: 2,
                    payload_base64: "first".to_owned(),
                },
                RawEventLog {
                    log_index: 3,
                    payload_base64: "second".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn ignores_data_emitted_by_nested_cpi_program() {
        let target = Pubkey::new_from_array([1; 32]);
        let child = Pubkey::new_from_array([2; 32]);
        let lines = logs(&[
            format!("Program {target} invoke [1]"),
            "Program data: before-cpi".to_owned(),
            format!("Program {child} invoke [2]"),
            "Program data: child-event".to_owned(),
            format!("Program {child} success"),
            "Program data: after-cpi".to_owned(),
            format!("Program {target} success"),
        ]);

        let events = extract_program_data(&lines, &target);

        assert_eq!(
            events,
            vec![
                RawEventLog {
                    log_index: 1,
                    payload_base64: "before-cpi".to_owned(),
                },
                RawEventLog {
                    log_index: 5,
                    payload_base64: "after-cpi".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn resumes_target_after_nested_program_failure() {
        let target = Pubkey::new_from_array([1; 32]);
        let child = Pubkey::new_from_array([2; 32]);
        let lines = logs(&[
            format!("Program {target} invoke [1]"),
            format!("Program {child} invoke [2]"),
            "Program data: child-event".to_owned(),
            format!("Program {child} failed: custom program error: 0x1"),
            "Program data: target-event".to_owned(),
            format!("Program {target} success"),
        ]);

        let events = extract_program_data(&lines, &target);

        assert_eq!(
            events,
            vec![RawEventLog {
                log_index: 4,
                payload_base64: "target-event".to_owned(),
            }]
        );
    }

    #[test]
    fn extracts_target_event_when_tidebook_is_invoked_through_cpi() {
        let target = Pubkey::new_from_array([1; 32]);
        let parent = Pubkey::new_from_array([2; 32]);
        let lines = logs(&[
            format!("Program {parent} invoke [1]"),
            format!("Program {target} invoke [2]"),
            "Program data: target-cpi-event".to_owned(),
            format!("Program {target} success"),
            format!("Program {parent} success"),
        ]);

        let events = extract_program_data(&lines, &target);

        assert_eq!(
            events,
            vec![RawEventLog {
                log_index: 2,
                payload_base64: "target-cpi-event".to_owned(),
            }]
        );
    }

    #[test]
    fn ignores_data_outside_target_execution() {
        let target = Pubkey::new_from_array([1; 32]);
        let unrelated = Pubkey::new_from_array([2; 32]);
        let lines = logs(&[
            "Program data: no-active-program".to_owned(),
            format!("Program {unrelated} invoke [1]"),
            "Program data: unrelated-event".to_owned(),
            format!("Program {unrelated} success"),
        ]);

        assert!(extract_program_data(&lines, &target).is_empty());
    }

    #[test]
    fn mismatched_completion_does_not_pop_active_program() {
        let target = Pubkey::new_from_array([1; 32]);
        let unrelated = Pubkey::new_from_array([2; 32]);
        let lines = logs(&[
            format!("Program {target} invoke [1]"),
            format!("Program {unrelated} success"),
            "Program data: still-target-event".to_owned(),
            format!("Program {target} success"),
        ]);

        let events = extract_program_data(&lines, &target);

        assert_eq!(
            events,
            vec![RawEventLog {
                log_index: 2,
                payload_base64: "still-target-event".to_owned(),
            }]
        );
    }
}
