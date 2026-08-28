use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{LensMode, TuiState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Focus(usize),
    All,
    Pause,
    Failures,
    Restart,
    Open,
    Enter,
    Slowest,
    Editor,
    Copy,
    Migrate,
    Revert,
    CycleLens,
    Mail,
    Jobs,
    Wire,
    Quit,
    None,
}

pub fn interpret(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Action::Up,
        KeyCode::Down | KeyCode::Char('j') => Action::Down,
        KeyCode::Char('0') => Action::All,
        KeyCode::Char(c) if c.is_ascii_digit() => Action::Focus((c as u8 - b'0') as usize),
        KeyCode::Char('p') => Action::Pause,
        KeyCode::Char('f') => Action::Failures,
        KeyCode::Char('r') => Action::Restart,
        KeyCode::Char('o') => Action::Open,
        KeyCode::Enter => Action::Enter,
        KeyCode::Char('s') => Action::Slowest,
        KeyCode::Char('e') => Action::Editor,
        KeyCode::Char('c') => Action::Copy,
        KeyCode::Char('m') => Action::Migrate,
        KeyCode::Char('M') => Action::Revert,
        KeyCode::Tab => Action::CycleLens,
        KeyCode::Char('E') => Action::Mail,
        KeyCode::Char('J') => Action::Jobs,
        KeyCode::Char('w') => Action::Wire,
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        _ => Action::None,
    }
}

pub fn apply(state: &mut TuiState, action: Action) {
    match action {
        Action::Up => {
            state.paused = true;
            let max = state.max_log_offset();
            state.log_offset = (state.log_offset + 1).min(max);
        }
        Action::Down => {
            if state.log_offset == 0 {
                state.paused = false;
            } else {
                state.log_offset -= 1;
            }
        }
        Action::Focus(n) => {
            if n >= 1 {
                if let Some(i) = n.checked_sub(1) {
                    if i < state.services.len() {
                        state.focus = Some(i);
                        state.log_offset = 0;
                        state.force_redraw = true;
                    }
                }
            }
        }
        Action::All => {
            state.focus = None;
            state.failures_only = false;
            state.log_offset = 0;
            state.force_redraw = true;
        }
        Action::Pause => state.paused = !state.paused,
        Action::Failures => {
            state.failures_only = !state.failures_only;
            state.log_offset = 0;
            state.force_redraw = true;
            state.tempo_query = if state.failures_only {
                "{ status=error }".into()
            } else {
                "{}".into()
            };
        }
        Action::Restart => {
            if let Some(name) = state.target_service().map(|s| s.name.clone()) {
                state.say(format!("restarting {name} …"));
            }
        }
        Action::Open => {
            if let Some(url) = state.target_service().map(|s| s.url.clone()) {
                state.say(format!("open {url}"));
            }
        }
        Action::Enter => {
            if let Some(t) = selected_trace(state) {
                state.selected_trace = Some(t);
                state.lens = LensMode::Trace;
            }
        }
        Action::Slowest => {
            state.tempo_query = "{ duration > 500ms }".into();
            state.failures_only = false;
            state.say("slowest in window");
        }
        Action::Editor | Action::Copy | Action::Migrate | Action::Revert => {}
        Action::CycleLens => {
            state.lens = match state.lens {
                LensMode::Service => LensMode::Trace,
                LensMode::Trace => LensMode::Mail,
                LensMode::Mail => LensMode::Jobs,
                LensMode::Jobs => LensMode::Service,
            };
        }
        Action::Mail => state.lens = LensMode::Mail,
        Action::Jobs => state.lens = LensMode::Jobs,
        Action::Wire => state.wide_wire = !state.wide_wire,
        Action::Quit => state.quit = true,
        Action::None => {}
    }
}

fn selected_trace(state: &TuiState) -> Option<String> {
    let traces = state.visible_traces();
    if traces.is_empty() {
        return None;
    }
    let i = state.log_offset.min(traces.len().saturating_sub(1));
    Some(traces[i].trace_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::dev::banner::DevUrls;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn digits_focus_and_zero_clears() {
        let mut state = TuiState::new("teryon", &DevUrls::defaults(true, true, true));
        apply(&mut state, interpret(key(KeyCode::Char('1'))));
        assert_eq!(state.focus, Some(0));
        apply(&mut state, interpret(key(KeyCode::Char('0'))));
        assert!(state.focus.is_none());
        assert!(!state.failures_only);
    }

    #[test]
    fn focus_and_all_reset_the_log_cursor() {
        let mut state = TuiState::new("teryon", &DevUrls::defaults(true, true, true));
        state.log_offset = 12;
        apply(&mut state, interpret(key(KeyCode::Char('2'))));
        assert_eq!(state.focus, Some(1));
        assert_eq!(state.log_offset, 0);
        assert!(state.force_redraw);
        state.force_redraw = false;
        state.log_offset = 4;
        apply(&mut state, interpret(key(KeyCode::Char('0'))));
        assert!(state.focus.is_none());
        assert_eq!(state.log_offset, 0);
        assert!(state.force_redraw);
    }

    #[test]
    fn q_esc_and_ctrl_c_quit() {
        assert_eq!(interpret(key(KeyCode::Char('q'))), Action::Quit);
        assert_eq!(interpret(key(KeyCode::Esc)), Action::Quit);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(interpret(ctrl_c), Action::Quit);
        let mut state = TuiState::new("x", &DevUrls::defaults(true, false, false));
        apply(&mut state, interpret(key(KeyCode::Char('q'))));
        assert!(state.quit);
    }

    #[test]
    fn p_pauses_f_filters() {
        let mut state = TuiState::new("x", &DevUrls::defaults(true, false, false));
        apply(&mut state, interpret(key(KeyCode::Char('p'))));
        assert!(state.paused);
        apply(&mut state, interpret(key(KeyCode::Char('f'))));
        assert!(state.failures_only);
    }

    #[test]
    fn r_toasts_restart_of_the_focused_service() {
        let mut state = TuiState::new("x", &DevUrls::defaults(true, true, true));
        apply(&mut state, interpret(key(KeyCode::Char('3'))));
        apply(&mut state, interpret(key(KeyCode::Char('r'))));
        let name = state.target_service().unwrap().name.clone();
        assert!(state.toast.contains(&name), "{}", state.toast);
        assert!(state.toast.contains("restarting"), "{}", state.toast);
    }

    #[test]
    fn o_toasts_the_target_url() {
        let mut state = TuiState::new("x", &DevUrls::defaults(true, false, false));
        apply(&mut state, interpret(key(KeyCode::Char('o'))));
        assert!(state.toast.contains("http://"), "{}", state.toast);
    }

    #[test]
    fn c_is_copy() {
        assert_eq!(interpret(key(KeyCode::Char('c'))), Action::Copy);
    }

    fn log(i: usize) -> crate::commands::dev::log::LogLine {
        crate::commands::dev::log::LogLine::new("api", format!("line-{i}"))
    }

    #[test]
    fn up_does_not_eat_lines_that_fit() {
        let mut state = TuiState::new("x", &DevUrls::defaults(true, false, false));
        state.log_view_height = 20;
        state.logs = (0..5).map(log).collect();
        apply(&mut state, interpret(key(KeyCode::Up)));
        assert!(state.paused);
        assert_eq!(state.log_offset, 0);
        apply(&mut state, interpret(key(KeyCode::Up)));
        assert_eq!(state.log_offset, 0);
    }

    #[test]
    fn up_scrolls_only_through_overflow_and_down_returns_to_follow() {
        let mut state = TuiState::new("x", &DevUrls::defaults(true, false, false));
        state.log_view_height = 4;
        state.logs = (0..10).map(log).collect();
        apply(&mut state, interpret(key(KeyCode::Up)));
        assert!(state.paused);
        assert_eq!(state.log_offset, 1);
        for _ in 0..20 {
            apply(&mut state, interpret(key(KeyCode::Up)));
        }
        assert_eq!(state.log_offset, 6);
        apply(&mut state, interpret(key(KeyCode::Down)));
        assert_eq!(state.log_offset, 5);
        while state.log_offset > 0 {
            apply(&mut state, interpret(key(KeyCode::Down)));
        }
        apply(&mut state, interpret(key(KeyCode::Down)));
        assert_eq!(state.log_offset, 0);
        assert!(!state.paused);
    }
}
