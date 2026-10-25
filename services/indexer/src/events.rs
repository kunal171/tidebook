//! Extraction and decoding of Tidebook events from Solana execution logs.

use solana_pubkey::Pubkey;

const PROGRAM_PREFIX: &str = "Program ";
const INVOKE_MARKER: &str = " invoke [";
const SUCCESS_SUFFIX: &str = " success";
const FAILED_MARKER: &str = " failed: ";
const PROGRAM_DATA_PREFIX: &str = "Program data: ";

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
/// at the top of that stack.
pub fn extract_program_data(
    logs: &[String],
    program_id: &Pubkey,
) -> Vec<RawEventLog> {
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