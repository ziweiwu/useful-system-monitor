//! Making a hostile process name safe to draw.

use std::borrow::Cow;

/// C0, DEL and C1 — Unicode's "Other, control" class (`\p{Cc}`).
fn is_control_cc(candidate: char) -> bool {
    let code_point = candidate as u32;
    code_point < 0x20 || code_point == 0x7f || (0x80..=0x9f).contains(&code_point)
}

/// Replaces control characters with a visible placeholder.
///
/// Defence in depth, not a fix for a live exploit — the distinction matters, so
/// here is what was actually measured.
///
/// A process chooses its own name, and any user can set a hostile one:
/// `exec -a $'malware\rSafari' sleep 60`. If such a byte reached the renderer it
/// would be genuinely bad: a control character counts as zero cells, so every
/// width and row budget would be computed as though it were absent, and the
/// terminal would still act on it — a carriage return returns the cursor to
/// column zero, so `malware\rSafari` draws as `Safari`, hiding both the real
/// name and the PID beside it. A spoof in the one tool whose job is showing you
/// what is running.
///
/// macOS `ps` prevents that today: it escapes every control byte on the way out
/// — newline to the four characters `\012`, ESC to `^[`, carriage return to
/// `^M` — verified with `od -c`, which reports zero raw 0x0D bytes for a process
/// named that way. (`cat -v` renders a raw CR as `^M` too, so it cannot tell the
/// two apart; that mistake is why this was briefly believed to be reachable.)
///
/// So this guards the collector boundary rather than a live hole: the trait is
/// implementable by anything, the mock is not bound by `ps`'s escaping, and that
/// escaping is an undocumented implementation detail rather than a contract.
///
/// A visible placeholder rather than a silent strip: a name containing something
/// strange should look like it does. `Cow` keeps the fast path allocation-free,
/// which matters because this runs for every process on every sample.
pub fn sanitize_text(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_control_cc) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .map(|ch| if is_control_cc(ch) { '·' } else { ch })
            .collect(),
    )
}
