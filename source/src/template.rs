// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Text worked out from a template a person writes: plain text with `%NAME%`
//! macros in it, grouped by `{ }` and `[ ]`, with `||` between choices.
//!
//! - A macro is replaced by its value. Names are the caller's, and case doesn't
//!   matter.
//! - `{ }` groups without printing, and `[ ]` prints its brackets.
//! - A group whose macros all come out empty is left out, with its text.
//! - `a || b` is `a`, or `b` when `a` comes out empty.
//! - Text between two parts shows only when both sides show a value. A choice
//!   that is only text counts as text here, which is what lets a separator
//!   stand in for a part that is missing.
//! - Tabs, line breaks and other control characters are ignored, spaces at either
//!   end of the template, a group or a choice are too, and a run of spaces
//!   shows as one.
//! - A backslash makes the character after it plain text: `\%`, `\{`, `\|`.
//!
//! Nothing here knows what the names mean or how long the text may be. The tab
//! text is one user (`tabtitle`), and anything else that names things from a
//! template can bring its own list of names.

use std::fmt;

// Deeper than anyone writes by hand, and shallow enough that the recursion
// can't run out of stack on a file somebody filled with braces.
const MAX_DEPTH: usize = 32;

/// A parsed template. Parse once, expand as often as the values change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
	root: Vec<Seq>,
}

// One choice: a run of text, macros and groups.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Seq {
	items: Vec<Item>,
	has_macro: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
	Text(String),
	Macro(usize),
	Group {
		brackets: bool,
		choices: Vec<Seq>,
		has_macro: bool,
	},
}

/// What is wrong with a template. `at` counts characters from 1, the way a
/// person counts along the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
	UnclosedMacro { at: usize },
	UnknownMacro { name: String },
	Unclosed { open: char, at: usize },
	Unopened { close: char, at: usize },
	TooDeep { at: usize },
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::UnclosedMacro { at } => {
				write!(f, "The % at character {at} has no closing %.")
			}
			Self::UnknownMacro { name } => write!(f, "%{name}% isn't a macro."),
			Self::Unclosed { open, at } => {
				write!(
					f,
					"The {open} at character {at} has no closing {}.",
					closer(*open)
				)
			}
			Self::Unopened { close, at } => {
				write!(
					f,
					"The {close} at character {at} has no opening {}.",
					opener(*close)
				)
			}
			Self::TooDeep { at } => {
				write!(f, "Groups go more than {MAX_DEPTH} deep at character {at}.")
			}
		}
	}
}

impl std::error::Error for Error {}

fn closer(open: char) -> char {
	if open == '[' { ']' } else { '}' }
}

fn opener(close: char) -> char {
	if close == ']' { '[' } else { '{' }
}

impl Template {
	/// Read `text`, knowing the macros named in `names`. A macro's index in
	/// `names` is where `expand` looks for its value.
	pub fn parse(text: &str, names: &[&str]) -> Result<Self, Error> {
		let mut parser = Parser {
			chars: text.chars().collect(),
			pos: 0,
			names,
		};
		let root = parser.choices(None, 0)?;
		Ok(Self { root })
	}

	/// The text, with each macro replaced by `values[index]`. A value missing
	/// from the slice is empty.
	pub fn expand(&self, values: &[&str]) -> String {
		match choose(&self.root, values) {
			Out::Value(text) | Out::Text(text) => text,
			Out::Gone => String::new(),
		}
	}

	/// Whether the macro at `index` appears anywhere in the template.
	pub fn uses(&self, index: usize) -> bool {
		fn in_seqs(seqs: &[Seq], index: usize) -> bool {
			seqs.iter().any(|seq| {
				seq.items.iter().any(|item| match item {
					Item::Macro(i) => *i == index,
					Item::Group { choices, .. } => in_seqs(choices, index),
					Item::Text(_) => false,
				})
			})
		}
		in_seqs(&self.root, index)
	}
}

struct Parser<'a> {
	chars: Vec<char>,
	pos: usize,
	names: &'a [&'a str],
}

impl Parser<'_> {
	// Choices up to the group's closer, or to the end at the top. `open` is the
	// group's opener and where it was, for the message when it never closes.
	fn choices(&mut self, open: Option<(char, usize)>, depth: usize) -> Result<Vec<Seq>, Error> {
		let mut choices = Vec::new();
		let mut items: Vec<Item> = Vec::new();
		let mut text = String::new();
		loop {
			let Some(&c) = self.chars.get(self.pos) else {
				if let Some((open, at)) = open {
					return Err(Error::Unclosed { open, at });
				}
				choices.push(seq(items, text));
				return Ok(choices);
			};
			let at = self.pos + 1;
			self.pos += 1;
			match c {
				'\\' => {
					// a backslash at the very end has nothing to make plain, so it is
					// plain itself
					let plain = self.chars.get(self.pos).copied().unwrap_or('\\');
					self.pos = (self.pos + 1).min(self.chars.len());
					push_char(&mut text, plain);
				}
				'%' => {
					let Some(len) = self.chars[self.pos..].iter().position(|&c| c == '%') else {
						return Err(Error::UnclosedMacro { at });
					};
					let name: String = self.chars[self.pos..self.pos + len].iter().collect();
					self.pos += len + 1;
					let Some(index) = self
						.names
						.iter()
						.position(|known| known.eq_ignore_ascii_case(name.trim()))
					else {
						return Err(Error::UnknownMacro { name });
					};
					flush(&mut items, &mut text);
					items.push(Item::Macro(index));
				}
				'{' | '[' => {
					if depth >= MAX_DEPTH {
						return Err(Error::TooDeep { at });
					}
					let choices = self.choices(Some((c, at)), depth + 1)?;
					let has_macro = choices.iter().any(|seq| seq.has_macro);
					flush(&mut items, &mut text);
					items.push(Item::Group {
						brackets: c == '[',
						choices,
						has_macro,
					});
				}
				'}' | ']' => match open {
					Some((open, _)) if closer(open) == c => {
						choices.push(seq(items, text));
						return Ok(choices);
					}
					_ => return Err(Error::Unopened { close: c, at }),
				},
				'|' if self.chars.get(self.pos) == Some(&'|') => {
					self.pos += 1;
					choices.push(seq(std::mem::take(&mut items), std::mem::take(&mut text)));
				}
				c => push_char(&mut text, c),
			}
		}
	}
}

// A run of spaces is one space, and every other control or white space
// character is nothing.
fn push_char(text: &mut String, c: char) {
	if c == ' ' {
		if !text.ends_with(' ') {
			text.push(' ');
		}
	} else if !c.is_control() && !c.is_whitespace() {
		text.push(c);
	}
}

fn flush(items: &mut Vec<Item>, text: &mut String) {
	if !text.is_empty() {
		items.push(Item::Text(std::mem::take(text)));
	}
}

// A finished choice, with the spaces at either end of it taken off.
fn seq(mut items: Vec<Item>, mut text: String) -> Seq {
	flush(&mut items, &mut text);
	if let Some(Item::Text(first)) = items.first_mut() {
		*first = first.trim_start().to_string();
	}
	if let Some(Item::Text(last)) = items.last_mut() {
		*last = last.trim_end().to_string();
	}
	items.retain(|item| !matches!(item, Item::Text(t) if t.is_empty()));
	let has_macro = items.iter().any(|item| match item {
		Item::Macro(_) => true,
		Item::Group { has_macro, .. } => *has_macro,
		Item::Text(_) => false,
	});
	Seq { items, has_macro }
}

// What a part came out as. A Value has a macro's value in it; Text is only
// text, from the template itself or from a choice with no macro in it; Gone is
// a part whose macros all came out empty.
#[derive(Debug)]
enum Out {
	Value(String),
	Text(String),
	Gone,
}

fn choose(choices: &[Seq], values: &[&str]) -> Out {
	for choice in choices {
		match expand_seq(choice, values) {
			Out::Gone => {}
			Out::Text(text) if text.trim().is_empty() => {}
			out => return out,
		}
	}
	if choices.iter().any(|choice| choice.has_macro) {
		Out::Gone
	} else {
		Out::Text(String::new())
	}
}

fn expand_seq(seq: &Seq, values: &[&str]) -> Out {
	let outs: Vec<Out> = seq
		.items
		.iter()
		.map(|item| expand_item(item, values))
		.collect();
	// Text with a missing part on one side and no value at all on that side is
	// a separator with nothing to separate.
	let side_ok = |side: &[Out]| {
		side.iter().any(|out| matches!(out, Out::Value(_)))
			|| !side.iter().any(|out| matches!(out, Out::Gone))
	};
	let mut text = String::new();
	let mut valued = false;
	let mut ends_in_text = false;
	for (i, out) in outs.iter().enumerate() {
		match out {
			Out::Value(value) => {
				text.push_str(value);
				valued = true;
				ends_in_text = false;
			}
			Out::Text(piece) if side_ok(&outs[..i]) && side_ok(&outs[i + 1..]) => {
				let piece = if text.is_empty() || text.ends_with(' ') {
					piece.trim_start()
				} else {
					piece
				};
				text.push_str(piece);
				ends_in_text |= !piece.is_empty();
			}
			Out::Text(_) | Out::Gone => {}
		}
	}
	if ends_in_text {
		text.truncate(text.trim_end().len());
	}
	if valued {
		Out::Value(text)
	} else if seq.has_macro && text.trim().is_empty() {
		Out::Gone
	} else {
		Out::Text(text)
	}
}

fn expand_item(item: &Item, values: &[&str]) -> Out {
	match item {
		Item::Text(text) => Out::Text(text.clone()),
		Item::Macro(index) => match values.get(*index) {
			Some(value) if !value.trim().is_empty() => Out::Value((*value).to_string()),
			_ => Out::Gone,
		},
		Item::Group {
			brackets, choices, ..
		} => {
			let wrap = |text: String| {
				if *brackets { format!("[{text}]") } else { text }
			};
			match choose(choices, values) {
				Out::Value(text) => Out::Value(wrap(text)),
				Out::Text(text) => Out::Text(wrap(text)),
				Out::Gone => Out::Gone,
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{Error, MAX_DEPTH, Template};

	const NAMES: [&str; 3] = ["A", "B", "C"];

	fn parse(text: &str) -> Template {
		match Template::parse(text, &NAMES) {
			Ok(template) => template,
			Err(e) => panic!("{text:?}: {e}"),
		}
	}

	fn run(text: &str, values: [&str; 3]) -> String {
		parse(text).expand(&values)
	}

	fn err(text: &str) -> Error {
		match Template::parse(text, &NAMES) {
			Ok(template) => panic!("{text:?} parsed as {template:?}"),
			Err(e) => e,
		}
	}

	// Test ID: Et0TY7y
	#[test]
	fn a_macro_is_replaced_by_its_value_whatever_case_it_is_written_in() {
		assert_eq!(run("%A%", ["x", "", ""]), "x");
		assert_eq!(run("%a%-%b%", ["x", "y", ""]), "x-y");
		assert_eq!(run("one %C% two", ["", "", "z"]), "one z two");
		assert!(parse("%c%").uses(2));
		assert!(!parse("%c%").uses(0));
		// a value goes in as it is, its own spaces too
		assert_eq!(run("%A%", ["  x  y ", "", ""]), "  x  y ");
	}

	// Test ID: Et0TY7z
	#[test]
	fn plain_text_has_its_spaces_tidied_and_nothing_else() {
		assert_eq!(run("hello", ["", "", ""]), "hello");
		assert_eq!(run("  a   b  ", ["", "", ""]), "a b");
		assert_eq!(run("a\tb\nc\r\n d", ["", "", ""]), "abc d");
		assert_eq!(run("a\u{1b}b\u{0}c", ["", "", ""]), "abc");
		assert_eq!(run("", ["x", "", ""]), "");
		assert_eq!(
			run("100\\% \\{x\\} \\[y\\] a\\|\\|b c|d", ["", "", ""]),
			"100% {x} [y] a||b c|d"
		);
		assert_eq!(run("end\\", ["", "", ""]), "end\\");
	}

	// Test ID: Et0TY80
	#[test]
	fn braces_group_without_printing_and_brackets_print() {
		assert_eq!(run("{ %A% }", ["x", "", ""]), "x");
		assert_eq!(run("[ %A% ]", ["x", "", ""]), "[x]");
		assert_eq!(run("[{ %A% }]", ["x", "", ""]), "[x]");
		assert_eq!(run("[plain]", ["", "", ""]), "[plain]");
		assert_eq!(run("{}", ["", "", ""]), "");
	}

	// Test ID: Et0TY81
	#[test]
	fn a_group_whose_macros_are_all_empty_goes_with_its_text() {
		assert_eq!(run("{ at %A% }", ["", "", ""]), "");
		assert_eq!(run("[last: %A%]", ["", "", ""]), "");
		assert_eq!(run("[last: %A%]", ["vim", "", ""]), "[last: vim]");
		assert_eq!(run("%C% {( %A% %B% )} y", ["", "", "x"]), "x y");
		// the whole template is a group too
		assert_eq!(run("x {( %A% %B% )} y", ["", "", ""]), "");
		assert_eq!(run("x {(%A% %B%)} y", ["", "b", ""]), "x (b) y");
		// a value that is only spaces is empty
		assert_eq!(run("[%A%]", [" ", "", ""]), "");
	}

	// Test ID: Et0TY82
	#[test]
	fn a_fallback_takes_the_first_choice_that_is_not_empty() {
		assert_eq!(run("%A% || %B% || %C%", ["a", "b", "c"]), "a");
		assert_eq!(run("%A% || %B% || %C%", ["", "b", "c"]), "b");
		assert_eq!(run("%A% || %B% || %C%", ["", "", "c"]), "c");
		assert_eq!(run("%A% || %B% || %C%", ["", "", ""]), "");
		assert_eq!(run("%A% || none", ["", "", ""]), "none");
		assert_eq!(
			run("[{ %A% || last: %B% }]", ["", "vim", ""]),
			"[last: vim]"
		);
		assert_eq!(run("[{ %A% || last: %B% }]", ["", "", ""]), "");
		// an empty choice is skipped like an empty macro
		assert_eq!(run("|| %A% ||", ["a", "", ""]), "a");
		// one bar is text
		assert_eq!(run("%A% | %B%", ["a", "b", ""]), "a | b");
	}

	// The rule that puts the dash in "Bash - ~/" only while no program is named.
	// Test ID: Et0TY83
	#[test]
	fn text_between_two_parts_shows_only_when_both_sides_do() {
		let dash = "%A% {[%B%] || -} %C%";
		assert_eq!(run(dash, ["sh", "vim", "~/"]), "sh [vim] ~/");
		assert_eq!(run(dash, ["sh", "", "~/"]), "sh - ~/");
		assert_eq!(run(dash, ["sh", "", ""]), "sh");
		assert_eq!(run(dash, ["", "", "~/"]), "~/");
		assert_eq!(run(dash, ["", "vim", ""]), "[vim]");
		assert_eq!(run(dash, ["sh", "vim", ""]), "sh [vim]");
		assert_eq!(run(dash, ["", "", ""]), "");
		// a missing part between two that show leaves one space
		assert_eq!(run("%A% %B% - %C%", ["sh", "", "~/"]), "sh - ~/");
		assert_eq!(run("%A% / %B%", ["a", "", ""]), "a");
		assert_eq!(run("%A% / %B%", ["", "b", ""]), "b");
		// text before the first part and after the last goes with its group
		assert_eq!(run("{on %A% now}", ["x", "", ""]), "on x now");
		// a choice that is only text stands in for a value at the front
		assert_eq!(run("{%A% || idle} - %B%", ["", "~/", ""]), "idle - ~/");
	}

	// Test ID: Et0TY84
	#[test]
	fn a_malformed_template_says_what_is_wrong_and_where() {
		assert_eq!(err("ab %A"), Error::UnclosedMacro { at: 4 });
		assert_eq!(err("%D%"), Error::UnknownMacro { name: "D".into() });
		assert_eq!(
			err("%%"),
			Error::UnknownMacro {
				name: String::new()
			}
		);
		assert_eq!(err("x { %A%"), Error::Unclosed { open: '{', at: 3 });
		assert_eq!(err("[[%A%]"), Error::Unclosed { open: '[', at: 1 });
		assert_eq!(err("%A% }"), Error::Unopened { close: '}', at: 5 });
		assert_eq!(err("{ %A% ]"), Error::Unopened { close: ']', at: 7 });
		let deep = "{".repeat(MAX_DEPTH + 1);
		assert_eq!(err(&deep), Error::TooDeep { at: MAX_DEPTH + 1 });
		// only the depth is limited, not the count
		assert!(Template::parse(&"{}".repeat(5000), &NAMES).is_ok());
		assert_eq!(
			err("x { %A%").to_string(),
			"The { at character 3 has no closing }."
		);
		assert_eq!(err("%Dir%").to_string(), "%Dir% isn't a macro.");
		// a name may have spaces round it, as a person might type it
		assert_eq!(run("% a %", ["x", "", ""]), "x");
	}

	// Whatever a config file holds, reading it and filling it in finishes, and
	// what comes out has no control characters in it.
	mod fuzz {
		use super::super::Template;
		use crate::fuzz;

		fn template(rng: &mut fuzz::Rng) -> Vec<u8> {
			#[rustfmt::skip]
			const PIECES: [&str; 18] = [
				"%A%", "%b%", "%C%", "%", "{", "}", "[", "]", "||", "|", "\\", " ",
				"-", "last: ", "\t", "\u{1b}", "%X%", "中",
			];
			let mut out = String::new();
			for _ in 0..rng.below(40) {
				out.push_str(rng.pick(&PIECES));
			}
			out.into_bytes()
		}

		fn check(case: &[u8], rng: &mut fuzz::Rng) {
			const VALUES: [&str; 4] = ["", " ", "x", "a b"];
			let text = String::from_utf8_lossy(case);
			let Ok(template) = Template::parse(&text, &["A", "B", "C"]) else {
				return;
			};
			let values = [*rng.pick(&VALUES), *rng.pick(&VALUES), *rng.pick(&VALUES)];
			let out = template.expand(&values);
			assert!(!out.chars().any(char::is_control), "{text:?} gave {out:?}");
			assert!(
				!out.starts_with(' ') && !out.ends_with(' '),
				"{text:?} gave {out:?}"
			);
		}

		// Test ID: Et0TY85
		#[test]
		fn any_template_reads_and_fills_in_without_a_panic() {
			let corpus = fuzz::corpus("template");
			let mut rng = fuzz::Rng::new(1);
			for case in &corpus {
				check(case, &mut rng);
			}
			let mut parsed = 0;
			fuzz::soak("template", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let case = fuzz::input(&mut rng, &corpus, template);
				if Template::parse(&String::from_utf8_lossy(&case), &["A", "B", "C"]).is_ok() {
					parsed += 1;
				}
				check(&case, &mut rng);
			});
			// the generator still makes templates that read
			assert!(parsed > 0);
		}
	}
}
