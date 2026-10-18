// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Closing a pane, tab or window that has a pane SilkTerm started in tmux:
//! detach, end the sessions, or ask, as `shell.tmux.on_close` says. Only a
//! close somebody asked for comes through here. A pane whose tmux client
//! exited, by a detach inside it or its session ending, just closes.

use super::{App, CloseScope, State, close_scope, save_live};
use crate::config;
use crate::term::PaneId;
use crate::tmux::{OnClose, Session};

/// What is being closed. A tab is named by one of its panes, since its place
/// in the strip can change while the question is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Closing {
	Pane(PaneId),
	Tab(PaneId),
	Window,
}

/// What a close does before it closes anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plan {
	Close,
	EndThenClose,
	Ask,
}

/// The setting decides only where there are sessions SilkTerm started.
pub(super) fn close_plan(on_close: OnClose, has_sessions: bool) -> Plan {
	match (has_sessions, on_close) {
		(false, _) | (true, OnClose::Detach) => Plan::Close,
		(true, OnClose::End) => Plan::EndThenClose,
		(true, OnClose::Prompt) => Plan::Ask,
	}
}

/// What "Don't ask me next time" stores for an answer.
pub(super) fn remembered(end: bool) -> OnClose {
	if end { OnClose::End } else { OnClose::Detach }
}

/// The question's window title and paragraphs.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(super) fn ask_text(closing: Closing, names: &[String]) -> (String, Vec<String>) {
	let (title, what) = match closing {
		Closing::Pane(_) => ("Close pane", "this pane"),
		Closing::Tab(_) => ("Close tab", "this tab"),
		Closing::Window => ("Close window", "this window"),
	};
	let quoted: Vec<String> = names.iter().map(|n| format!("\"{n}\"")).collect();
	let first = match quoted.as_slice() {
		[] => format!("SilkTerm started a tmux session in {what}."),
		[one] => format!("SilkTerm started the tmux session {one} in {what}."),
		[a, b] => format!("SilkTerm started 2 tmux sessions in {what}: {a} and {b}."),
		[many @ .., last] => format!(
			"SilkTerm started {} tmux sessions in {what}: {}, and {last}.",
			quoted.len(),
			many.join(", ")
		),
	};
	let it = if names.len() > 1 { "them" } else { "it" };
	(
		title.to_string(),
		vec![
			first,
			format!(
				"Detach leaves {it} running, so you can attach to {it} again later. End closes {it}, and everything running in {it}."
			),
		],
	)
}

impl State {
	fn sessions_closing(&self, closing: Closing) -> Vec<Session> {
		let tabs = &self.tabs.list;
		match closing {
			Closing::Pane(id) => tabs
				.iter()
				.find_map(|pm| pm.panes.get(&id))
				.and_then(|pane| pane.tmux.clone())
				.into_iter()
				.collect(),
			Closing::Tab(anchor) => tabs
				.iter()
				.find(|pm| pm.panes.contains_key(&anchor))
				.map(crate::pane::PaneManager::tmux_sessions)
				.unwrap_or_default(),
			Closing::Window => tabs
				.iter()
				.flat_map(crate::pane::PaneManager::tmux_sessions)
				.collect(),
		}
	}

	/// True when the close can go ahead now. With End set, the sessions are
	/// ended first. With Prompt set, the question is put up instead, and the
	/// close waits on its answer.
	pub(super) fn may_close(&mut self, closing: Closing) -> bool {
		let sessions = self.sessions_closing(closing);
		match close_plan(config::settings().tmux_on_close, !sessions.is_empty()) {
			Plan::Close => true,
			Plan::EndThenClose => {
				sessions.iter().for_each(crate::tmux::end);
				true
			}
			Plan::Ask => {
				self.close_ask = Some(closing);
				false
			}
		}
	}

	/// The question was answered Detach or End. What is closed is looked up
	/// again, since a pane may have gone while it was up.
	pub(super) fn answer_close(&mut self, closing: Closing, end: bool, remember: bool) {
		if end {
			self.sessions_closing(closing)
				.iter()
				.for_each(crate::tmux::end);
		}
		if remember {
			let word = knobs::Value::Text(config::Choice::key(remembered(end)).to_string());
			save_live(|live| {
				crate::fields::set(live, crate::ui_spec::Key::TmuxOnClose, &word, None);
				true
			});
		}
		match closing {
			Closing::Pane(id) => self.close_pane_now(id),
			Closing::Tab(anchor) => {
				if let Some(i) = self
					.tabs
					.list
					.iter()
					.position(|pm| pm.panes.contains_key(&anchor))
				{
					self.close_tab_at(i);
				}
			}
			Closing::Window => self.quit = true,
		}
		self.dirty = true;
	}

	/// Close one pane of the shown tab, then its tab, then the window, as the
	/// Panes menu's Close pane does.
	pub(super) fn close_pane_now(&mut self, target: PaneId) {
		let area = self.area();
		let Some(tab) = self
			.tabs
			.list
			.iter()
			.position(|pm| pm.panes.contains_key(&target))
		else {
			return;
		};
		let scope = close_scope(true, self.tabs.list[tab].panes.len(), self.tabs.len());
		match scope {
			CloseScope::Pane => {
				self.tabs.list[tab].close(&mut self.text, target, area);
			}
			// last pane in its tab -> the tab; last pane of the last tab -> the window
			CloseScope::Tab => self.close_tab_at(tab),
			CloseScope::Window => self.quit = true,
			CloseScope::Nothing => {}
		}
	}

	/// The tab's close button, or Close tab from a menu or hotkey.
	pub(super) fn request_close_tab(&mut self, idx: usize) {
		let Some(anchor) = self.tabs.list.get(idx).map(|pm| pm.focused) else {
			return;
		};
		if self.may_close(Closing::Tab(anchor)) {
			self.close_tab_at(idx);
		}
	}

	/// Quit, or the window's own close button.
	pub(super) fn request_quit(&mut self) {
		if self.may_close(Closing::Window) {
			self.quit = true;
		}
	}
}

impl App {
	/// Put up a question a close is waiting on, once nothing else holds the
	/// notice's place. Windows has no tmux, so nothing there asks.
	pub(super) fn show_close_ask(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
		if self.notice.is_some() {
			return;
		}
		let Some(state) = self.state.as_mut() else {
			return;
		};
		let Some(closing) = state.close_ask.take() else {
			return;
		};
		// a template that starts with the time puts them in the order made
		let mut names: Vec<String> = state
			.sessions_closing(closing)
			.into_iter()
			.map(|s| s.name)
			.collect();
		names.sort();
		// the panes it was about went while it waited
		if names.is_empty() {
			state.answer_close(closing, false, false);
			return;
		}
		#[cfg(target_os = "windows")]
		{
			let _ = event_loop;
			state.answer_close(closing, false, false);
		}
		#[cfg(not(target_os = "windows"))]
		{
			use winit::raw_window_handle::HasWindowHandle;
			let (title, paras) = ask_text(closing, &names);
			let parent = state.window.window_handle().ok().map(|h| h.as_raw());
			let warm = self.gpu_warm.get();
			match crate::dialog::DialogWin::new_ask(
				event_loop,
				title,
				&paras,
				parent,
				warm.as_ref(),
			) {
				Ok(n) => {
					self.notice = Some(n);
					self.notice_dirty = true;
					self.asking = Some(closing);
				}
				// with no way to ask, nothing is ended
				Err(e) => {
					eprintln!("{}: question window failed: {e}", config::APP_NAME);
					state.answer_close(closing, false, false);
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: Et0ZjvK
	#[test]
	fn the_setting_decides_only_where_tmux_sessions_would_go() {
		for on_close in [OnClose::Detach, OnClose::End, OnClose::Prompt] {
			assert_eq!(close_plan(on_close, false), Plan::Close, "{on_close:?}");
		}
		assert_eq!(close_plan(OnClose::Detach, true), Plan::Close);
		assert_eq!(close_plan(OnClose::End, true), Plan::EndThenClose);
		assert_eq!(close_plan(OnClose::Prompt, true), Plan::Ask);
	}

	// The box stores the answer given, so the next close does it without
	// asking, and a stored answer never asks again.
	// Test ID: Et0ZjyN
	#[test]
	fn dont_ask_stores_the_answer_given() {
		assert_eq!(remembered(true), OnClose::End);
		assert_eq!(remembered(false), OnClose::Detach);
		for end in [true, false] {
			assert_ne!(close_plan(remembered(end), true), Plan::Ask);
		}
		assert_eq!(close_plan(remembered(true), true), Plan::EndThenClose);
	}

	// Test ID: Et0Zk1A
	#[test]
	fn the_question_names_what_closes_and_each_session() {
		let one = ["20261017-17332704".to_string()];
		let (title, paras) = ask_text(Closing::Tab(7), &one);
		assert_eq!(title, "Close tab");
		assert_eq!(
			paras[0],
			"SilkTerm started the tmux session \"20261017-17332704\" in this tab."
		);
		assert!(paras[1].starts_with("Detach leaves it running"));
		let three = ["a".to_string(), "b".to_string(), "c".to_string()];
		let (title, paras) = ask_text(Closing::Window, &three);
		assert_eq!(title, "Close window");
		assert_eq!(
			paras[0],
			"SilkTerm started 3 tmux sessions in this window: \"a\", \"b\", and \"c\"."
		);
		assert!(paras[1].contains("leaves them running"));
		assert_eq!(ask_text(Closing::Pane(1), &one).0, "Close pane");
	}
}
