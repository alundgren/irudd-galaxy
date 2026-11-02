//! Moving the play window between monitors with different scales.
//!
//! When a window crosses onto a monitor with another scale (100% to 150%, say), Windows
//! sends `WM_DPICHANGED` with a rectangle for the window on the new monitor: the same
//! logical size, placed so the pointer keeps its grip on the title bar. winit 0.30
//! throws that rectangle away and computes its own, then "nudges" it back onto the
//! monitor it believes is right. On Windows 11 that check is wrong, so the nudge drags
//! the window back across the edge, which changes the scale again: the window grows
//! sideways and fights the drag (winit issue 4041, fixed upstream after 0.30).
//!
//! The fix keeps winit's handling (it updates its scale and sends `ScaleFactorChanged`,
//! which the renderer follows) and corrects only where winit puts the window: while
//! winit handles `WM_DPICHANGED`, its own `SetWindowPos` is moved to the top-left corner
//! Windows suggested. winit's size is kept: it keeps the client area's logical size
//! exact, while Windows scales the whole outer rectangle, frame included, which left a
//! 1280x720 window 1278x719 after a move from 125% to 100%. [`ScaleMove`] is that rule,
//! kept free of Windows types so it is tested on every platform; the Windows-only part
//! is a message hook on the window.
//! Other platforms need nothing: macOS, X11 and Wayland keep the logical size
//! themselves, and the renderer follows `ScaleFactorChanged` there too.

/// A window rectangle in physical screen pixels, frame included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A window move about to happen (Windows' `WINDOWPOS`): where to, and whether it
/// moves or sizes at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub rect: Rect,
    pub moves: bool,
    pub sizes: bool,
}

/// The rectangles Windows suggested for the scale changes being handled, innermost
/// last. A scale change can start another inside it, if a move lands on yet another
/// monitor.
#[derive(Default, Debug)]
pub struct ScaleMove {
    suggested: Vec<Rect>,
}

impl ScaleMove {
    /// A scale change starts, with the rectangle Windows suggests.
    pub fn begin(&mut self, suggested: Rect) {
        self.suggested.push(suggested);
    }

    /// The scale change has been handled.
    pub fn end(&mut self) {
        self.suggested.pop();
    }

    /// The move to make instead of `requested`: during a scale change, a move or resize
    /// goes to the suggested top-left corner, at the size requested (winit's, which keeps
    /// the logical size), or the suggested size if the request had none. Outside a scale
    /// change, or for a change that neither moves nor sizes the window (a style change),
    /// nothing changes.
    pub fn correct(&self, requested: Move) -> Option<Move> {
        let suggested = *self.suggested.last()?;
        if !requested.moves && !requested.sizes {
            return None;
        }
        let (width, height) = if requested.sizes {
            (requested.rect.width, requested.rect.height)
        } else {
            (suggested.width, suggested.height)
        };
        Some(Move {
            rect: Rect {
                x: suggested.x,
                y: suggested.y,
                width,
                height,
            },
            moves: true,
            sizes: true,
        })
    }
}

#[cfg(windows)]
pub use hook::Hook;

#[cfg(windows)]
#[allow(unsafe_code)]
mod hook {
    use super::{Move, Rect, ScaleMove};
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_DPICHANGED, WM_WINDOWPOSCHANGING,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// Any number unique to this hook on the window.
    const ID: usize = 0x6761_6c78;

    /// The hook on one window. Dropping it unhooks the window; drop it before the window.
    pub struct Hook {
        hwnd: HWND,
        /// Read by the hook procedure through the pointer it was installed with.
        _state: Box<RefCell<ScaleMove>>,
    }

    impl Hook {
        /// Hooks `window`, or returns `None` (and the window keeps winit's handling) if
        /// Windows refuses.
        pub fn install(window: &Window) -> Option<Hook> {
            let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
                return None;
            };
            let hwnd = handle.hwnd.get() as HWND;
            let state = Box::new(RefCell::new(ScaleMove::default()));
            let data = &*state as *const RefCell<ScaleMove> as usize;
            // SAFETY: `hwnd` is the live window, on this (its own) thread. `data` points
            // into `state`, which the hook owns and keeps until it removes the subclass.
            let ok = unsafe { SetWindowSubclass(hwnd, Some(procedure), ID, data) };
            (ok != 0).then_some(Hook {
                hwnd,
                _state: state,
            })
        }
    }

    impl Drop for Hook {
        fn drop(&mut self) {
            // SAFETY: the subclass was installed on this window with this procedure and ID.
            unsafe { RemoveWindowSubclass(self.hwnd, Some(procedure), ID) };
        }
    }

    fn rect(r: &RECT) -> Rect {
        Rect {
            x: r.left,
            y: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
        }
    }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        // SAFETY: `data` is the hook's `RefCell<ScaleMove>`, alive while the subclass is.
        let state = unsafe { &*(data as *const RefCell<ScaleMove>) };
        match msg {
            WM_DPICHANGED => {
                // SAFETY: WM_DPICHANGED's lparam points at the suggested RECT.
                let suggested = rect(unsafe { &*(lparam as *const RECT) });
                state.borrow_mut().begin(suggested);
                // winit handles it as usual; its SetWindowPos comes back through
                // WM_WINDOWPOSCHANGING below, while the suggestion is set.
                let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
                state.borrow_mut().end();
                result
            }
            WM_WINDOWPOSCHANGING => {
                // SAFETY: WM_WINDOWPOSCHANGING's lparam points at a WINDOWPOS the
                // receiver may change.
                let pos = unsafe { &mut *(lparam as *mut WINDOWPOS) };
                let requested = Move {
                    rect: Rect {
                        x: pos.x,
                        y: pos.y,
                        width: pos.cx,
                        height: pos.cy,
                    },
                    moves: pos.flags & SWP_NOMOVE == 0,
                    sizes: pos.flags & SWP_NOSIZE == 0,
                };
                let corrected = state.borrow().correct(requested);
                if let Some(m) = corrected {
                    pos.x = m.rect.x;
                    pos.y = m.rect.y;
                    pos.cx = m.rect.width;
                    pos.cy = m.rect.height;
                    pos.flags &= !(SWP_NOMOVE | SWP_NOSIZE);
                }
                unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
            }
            _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUGGESTED: Rect = Rect {
        x: 1700,
        y: 200,
        width: 1936,
        height: 1127,
    };

    fn winit_move() -> Move {
        // winit's own placement: its size, nudged back towards the old monitor.
        Move {
            rect: Rect {
                x: 1380,
                y: 200,
                width: 1934,
                height: 1124,
            },
            moves: true,
            sizes: true,
        }
    }

    #[test]
    fn during_a_scale_change_the_window_goes_where_windows_suggested_at_winits_size() {
        let mut s = ScaleMove::default();
        s.begin(SUGGESTED);
        let m = s.correct(winit_move()).unwrap();
        // Windows' corner, so the window isn't pushed back across the edge; winit's
        // size, which keeps the client area's logical size exact.
        let expected = Rect {
            x: 1700,
            y: 200,
            width: 1934,
            height: 1124,
        };
        assert_eq!(m.rect, expected);
        assert!(m.moves && m.sizes);
        // A resize alone still lands on the corner; a move alone takes Windows' size.
        let only_size = Move {
            moves: false,
            ..winit_move()
        };
        assert_eq!(s.correct(only_size).unwrap().rect, expected);
        let only_move = Move {
            sizes: false,
            ..winit_move()
        };
        assert_eq!(s.correct(only_move).unwrap().rect, SUGGESTED);
        s.end();
        assert_eq!(
            s.correct(winit_move()),
            None,
            "ordinary moves are left alone"
        );
    }

    #[test]
    fn style_changes_and_moves_outside_a_scale_change_are_left_alone() {
        let mut s = ScaleMove::default();
        assert_eq!(s.correct(winit_move()), None);
        s.begin(SUGGESTED);
        let style = Move {
            moves: false,
            sizes: false,
            ..winit_move()
        };
        assert_eq!(s.correct(style), None);
    }

    #[test]
    fn a_scale_change_inside_another_uses_its_own_rectangle_then_the_outer_one() {
        let mut s = ScaleMove::default();
        s.begin(SUGGESTED);
        let inner = Rect {
            x: 10,
            y: 20,
            width: 1280,
            height: 751,
        };
        s.begin(inner);
        assert_eq!(s.correct(winit_move()).unwrap().rect.x, inner.x);
        s.end();
        assert_eq!(s.correct(winit_move()).unwrap().rect.x, SUGGESTED.x);
        s.end();
        assert_eq!(s.correct(winit_move()), None);
        s.end(); // An unmatched end is harmless.
    }
}
