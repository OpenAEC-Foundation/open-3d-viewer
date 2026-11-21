//! Holds the cursor where a look-around drag began, as a game does: the cursor is hidden, each
//! movement is read from where the cursor went and the cursor is put back, so the view turns on
//! as long as the mouse moves and the cursor never reaches the edge of the screen. The movement
//! is read from the cursor itself, not from the mouse events: those that were already waiting
//! when the cursor was put back would count a fast movement twice. On Windows; elsewhere (and in
//! the tests, which must not move the real cursor) the cursor moves freely and the drag looks
//! around as far as it reaches.

/// Where a look-around drag holds the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// On the screen, in physical pixels.
    at: (i32, i32),
    /// Physical pixels per logical pixel of the window.
    scale: f32,
}

/// The cursor where it is now, when it can be held there.
#[cfg(all(windows, not(test)))]
pub fn hold() -> Option<Anchor> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};
    let mut at = POINT::default();
    // SAFETY: `at` is a valid POINT to write to; the window handle is only read.
    unsafe { GetCursorPos(&mut at) }.ok()?;
    let dpi = unsafe { GetDpiForWindow(GetForegroundWindow()) };
    let scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
    Some(Anchor { at: (at.x, at.y), scale })
}

/// How far (logical pixels) the cursor moved from `anchor`, putting it back there.
#[cfg(all(windows, not(test)))]
pub fn take(anchor: Anchor) -> Option<(f32, f32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};
    let mut at = POINT::default();
    // SAFETY: `at` is a valid POINT to write to; SetCursorPos takes no pointers.
    unsafe { GetCursorPos(&mut at) }.ok()?;
    let moved = (at.x - anchor.at.0, at.y - anchor.at.1);
    if moved != (0, 0) {
        let _ = unsafe { SetCursorPos(anchor.at.0, anchor.at.1) };
    }
    Some((moved.0 as f32 / anchor.scale, moved.1 as f32 / anchor.scale))
}

#[cfg(not(all(windows, not(test))))]
pub fn hold() -> Option<Anchor> {
    None
}

#[cfg(not(all(windows, not(test))))]
pub fn take(_anchor: Anchor) -> Option<(f32, f32)> {
    None
}
