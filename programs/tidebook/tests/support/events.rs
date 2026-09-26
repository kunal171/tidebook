//! Anchor event decoding from LiteSVM transaction logs.

use {
    anchor_lang::{AnchorDeserialize, Event},
    base64::{engine::general_purpose::STANDARD, Engine as _},
    litesvm::types::TransactionResult,
};

/// Decodes all events of type `T` from a successful transaction in log order.
pub fn events<T>(result: &TransactionResult) -> Vec<T>
where
    T: Event + AnchorDeserialize,
{
    let metadata = result
        .as_ref()
        .unwrap_or_else(|failure| panic!("transaction failed: {failure:?}"));

    metadata
        .logs
        .iter()
        .filter_map(|log| log.strip_prefix("Program data: "))
        .filter_map(|payload| STANDARD.decode(payload).ok())
        .filter_map(|data| {
            let discriminator = T::DISCRIMINATOR;
            data.strip_prefix(discriminator)
                .and_then(|payload| T::deserialize(&mut &payload[..]).ok())
        })
        .collect()
}

/// Decodes exactly one event of type `T` from a successful transaction.
///
/// Anchor's `emit!` writes `discriminator || borsh_payload` as a base64
/// `Program data:` log. Other program-data logs are ignored by comparing the
/// discriminator before deserialization.
pub fn single_event<T>(result: &TransactionResult) -> T
where
    T: Event + AnchorDeserialize,
{
    let events = events::<T>(result);

    assert_eq!(
        events.len(),
        1,
        "expected exactly one {} event",
        core::any::type_name::<T>(),
    );

    events.into_iter().next().unwrap()
}
