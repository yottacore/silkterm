// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Windows file associations: what opens a .bat, .cmd, .ps1 or .vbs from
//! Explorer, and the folder menu's "Open in SilkTerm" entry. Per user only.
//!
//! Windows' own default terminal setting was ruled out: it hands over a console
//! session that is already running, through COM, and needs Windows Terminal
//! installed to do it. An association just starts SilkTerm with the file.
//!
//! Registering overrides one value per file type in HKCU, over the system's
//! entry in HKLM. Windows reads the two as one tree, and a value HKCU does not
//! have still comes from HKLM, so only the open command changes. What each
//! value held before is saved under `Software\SilkTerm\Associations`, which is
//! how "put back" can restore it exactly. A value somebody else changed after
//! us is theirs, and put back leaves it alone.
//!
//! Everything goes through `Store`, so the rules are tested here against a map.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Assoc {
	Batch,
	PowerShell,
	VbScript,
	Folder,
}

impl Assoc {
	pub const ALL: [Assoc; 4] = [
		Assoc::Batch,
		Assoc::PowerShell,
		Assoc::VbScript,
		Assoc::Folder,
	];

	fn name(self) -> &'static str {
		match self {
			Assoc::Batch => "Batch",
			Assoc::PowerShell => "PowerShell",
			Assoc::VbScript => "VBScript",
			Assoc::Folder => "Folder",
		}
	}

	// The file types, with the program ID Windows ships for each. The ID is read
	// from HKCR at registration, and this is only the fallback.
	fn types(self) -> &'static [(&'static str, &'static str)] {
		match self {
			Assoc::Batch => &[(".bat", "batfile"), (".cmd", "cmdfile")],
			Assoc::PowerShell => &[(".ps1", "Microsoft.PowerShellScript.1")],
			Assoc::VbScript => &[(".vbs", "VBSFile")],
			Assoc::Folder => &[],
		}
	}
}

pub const REG_NONE: u32 = 0;
pub const REG_SZ: u32 = 1;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Value {
	pub kind: u32,
	pub text: String,
}

impl Value {
	fn sz(text: impl Into<String>) -> Value {
		Value {
			kind: REG_SZ,
			text: text.into(),
		}
	}
}

/// Paths start with the hive, `HKCU\` or `HKCR\`. Only HKCU is ever written.
pub trait Store {
	fn get(&self, key: &str, name: &str) -> Option<Value>;
	fn set(&mut self, key: &str, name: &str, value: &Value) -> Result<(), String>;
	fn delete(&mut self, key: &str, name: &str) -> Result<(), String>;
	fn exists(&self, key: &str) -> bool;
	// Remove the key if nothing is left in it. Not an error when it is gone
	// already or still holds something.
	fn prune(&mut self, key: &str) -> Result<(), String>;
}

const STATE: &str = r"HKCU\Software\SilkTerm\Associations";
const CLASSES: &str = r"HKCU\Software\Classes";

// What one registration writes: the key, the value name ("" for the default)
// and the value.
struct Entry {
	key: String,
	name: String,
	value: Value,
}

fn entry(key: String, name: &str, value: Value) -> Entry {
	Entry {
		key,
		name: name.to_string(),
		value,
	}
}

fn quoted(exe: &Path) -> String {
	format!("\"{}\"", exe.display())
}

// The command a double-click runs. `--open` takes everything after it, so the
// file's own arguments (`%*`) reach the script.
fn open_command(exe: &Path) -> String {
	format!("{} --keep-open --open \"%1\" %*", quoted(exe))
}

fn entries(assoc: Assoc, exe: &Path, store: &dyn Store) -> Vec<Entry> {
	let mut out = Vec::new();
	if assoc == Assoc::Folder {
		let command = format!("{} --directory \"%V\"", quoted(exe));
		for place in [
			r"Directory\shell",
			r"Directory\Background\shell",
			r"Drive\shell",
		] {
			let verb = format!(r"{CLASSES}\{place}\SilkTerm");
			out.push(entry(verb.clone(), "", Value::sz("Open in SilkTerm")));
			out.push(entry(
				verb.clone(),
				"Icon",
				Value::sz(format!("{},0", quoted(exe))),
			));
			out.push(entry(
				format!(r"{verb}\command"),
				"",
				Value::sz(command.clone()),
			));
		}
		return out;
	}
	for &(ext, stock) in assoc.types() {
		let prog_id = prog_id_of(ext, stock, store);
		let verb = format!(r"{CLASSES}\{prog_id}\shell\open");
		out.push(entry(verb.clone(), "", Value::sz("Open in SilkTerm")));
		out.push(entry(
			format!(r"{verb}\command"),
			"",
			Value::sz(open_command(exe)),
		));
		// SilkTerm's own entry under Open with, which is the only way in when the
		// user has picked an app for the type there: Windows guards that choice.
		let own = format!("SilkTerm{ext}");
		out.push(entry(
			format!(r"{CLASSES}\{own}\shell\open\command"),
			"",
			Value::sz(open_command(exe)),
		));
		out.push(entry(
			format!(r"{CLASSES}\{ext}\OpenWithProgids"),
			&own,
			Value {
				kind: REG_NONE,
				text: String::new(),
			},
		));
	}
	out
}

fn prog_id_of(ext: &str, stock: &str, store: &dyn Store) -> String {
	store
		.get(&format!(r"HKCR\{ext}"), "")
		.map(|v| v.text.trim().to_string())
		.filter(|id| !id.is_empty() && !id.contains(['\\', '/']))
		.unwrap_or_else(|| stock.to_string())
}

fn state_key(assoc: Assoc) -> String {
	format!(r"{STATE}\{}", assoc.name())
}

pub fn registered(assoc: Assoc, store: &dyn Store) -> bool {
	store.exists(&state_key(assoc))
}

/// The file types whose double-click will still go elsewhere after registering,
/// because the user picked an app for them under Open with. Windows keeps that
/// choice where no program may write it, so all that can be done is say so.
pub fn overridden(assoc: Assoc, store: &dyn Store) -> Vec<&'static str> {
	assoc
		.types()
		.iter()
		.filter(|&&(ext, stock)| {
			let choice = format!(
				r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{ext}\UserChoice"
			);
			store.get(&choice, "ProgId").is_some_and(|picked| {
				let picked = picked.text.trim();
				!picked.eq_ignore_ascii_case(&prog_id_of(ext, stock, store))
					&& !picked.eq_ignore_ascii_case(&format!("SilkTerm{ext}"))
			})
		})
		.map(|&(ext, _)| ext)
		.collect()
}

/// Register, or register again: a second call puts back what the first saved
/// and saves it over, so the path follows the build that was last asked.
pub fn register(assoc: Assoc, exe: &Path, store: &mut dyn Store) -> Result<(), String> {
	if registered(assoc, store) {
		unregister(assoc, store)?;
	}
	let state = state_key(assoc);
	let todo = entries(assoc, exe, store);
	for (i, e) in todo.iter().enumerate() {
		// the first key on the way down that is not there yet is ours to remove
		let made = first_missing(&e.key, store);
		let before = store.get(&e.key, &e.name);
		store.set(&state, &format!("key{i}"), &Value::sz(&e.key))?;
		store.set(&state, &format!("name{i}"), &Value::sz(&e.name))?;
		store.set(&state, &format!("mine{i}"), &e.value)?;
		if let Some(before) = &before {
			store.set(&state, &format!("prev{i}"), before)?;
		}
		if let Some(made) = &made {
			store.set(&state, &format!("made{i}"), &Value::sz(made))?;
		}
		store.set(&e.key, &e.name, &e.value)?;
	}
	store.set(&state, "count", &Value::sz(todo.len().to_string()))
}

/// Put back what registering replaced, newest first so a key made for a value
/// is empty by the time its turn comes.
pub fn unregister(assoc: Assoc, store: &mut dyn Store) -> Result<(), String> {
	let state = state_key(assoc);
	let text = |store: &dyn Store, name: String| store.get(&state, &name).map(|v| v.text);
	let count: usize = text(store, "count".into())
		.and_then(|n| n.parse().ok())
		.unwrap_or(0);
	for i in (0..count).rev() {
		let (Some(key), Some(name)) = (
			text(store, format!("key{i}")),
			text(store, format!("name{i}")),
		) else {
			continue;
		};
		let mine = store.get(&state, &format!("mine{i}"));
		if store.get(&key, &name) == mine {
			match store.get(&state, &format!("prev{i}")) {
				Some(before) => store.set(&key, &name, &before)?,
				None => store.delete(&key, &name)?,
			}
		}
		if let Some(made) = text(store, format!("made{i}")) {
			let mut at = key.as_str();
			loop {
				store.prune(at)?;
				if at.len() <= made.len() {
					break;
				}
				match at.rfind('\\') {
					Some(cut) => at = &at[..cut],
					None => break,
				}
			}
		}
	}
	// state first, then the two parents it may have been the last thing in
	for name in
		["count"]
			.into_iter()
			.map(String::from)
			.chain((0..count).flat_map(|i| {
				["key", "name", "mine", "prev", "made"].map(|field| format!("{field}{i}"))
			})) {
		if store.get(&state, &name).is_some() {
			store.delete(&state, &name)?;
		}
	}
	store.prune(&state)?;
	store.prune(STATE)?;
	store.prune(r"HKCU\Software\SilkTerm")
}

fn first_missing(key: &str, store: &dyn Store) -> Option<String> {
	let mut missing = None;
	let mut at = key;
	// never above the classes root: that one always exists
	while at.len() > CLASSES.len() && !store.exists(at) {
		missing = Some(at.to_string());
		at = &at[..at.rfind('\\')?];
	}
	missing
}

/// The program an association names. A dogfood build runs from a versions
/// folder whose copies are renamed as they age, and the launcher keeps a link
/// beside that folder pointed at the newest. Naming the link is what keeps the
/// association working after the next build.
pub fn exe_to_register(current: &Path, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
	let versions = current.parent();
	if let Some(dir) = versions.filter(|d| d.file_name().is_some_and(|n| n == "silkterm_versions"))
	{
		if let Some(link) = dir.parent().map(|up| up.join("silkterm.exe")) {
			if exists(&link) {
				return link;
			}
		}
	}
	current.to_path_buf()
}

/// The argv `--open` runs a file with. A batch file is started as itself:
/// `CreateProcess` hands a .bat or .cmd to cmd.exe with the quoting cmd wants,
/// which is what Explorer does too. Anything else runs directly.
/// `pwsh` is asked only for a .ps1, since it searches PATH.
pub fn open_argv(
	file: &str,
	args: &[String],
	windows: bool,
	pwsh: &dyn Fn() -> bool,
) -> Vec<String> {
	let ext = Path::new(file)
		.extension()
		.map(|e| e.to_string_lossy().to_ascii_lowercase())
		.unwrap_or_default();
	let mut argv = Vec::new();
	match ext.as_str() {
		"ps1" if windows => {
			// The same policy step as Windows' own "Run with PowerShell": a
			// script runs unless the machine insists on signed ones.
			let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
			let mut script = format!(
				"if((Get-ExecutionPolicy) -ne 'AllSigned') {{ Set-ExecutionPolicy -Scope Process Bypass }}; & {}",
				quote(file)
			);
			for arg in args {
				script.push(' ');
				script.push_str(&quote(arg));
			}
			let host = if pwsh() { "pwsh.exe" } else { "powershell.exe" };
			argv.extend([host, "-NoLogo", "-Command"].map(String::from));
			argv.push(script);
			return argv;
		}
		// the console script host, so output goes to the pane rather than to
		// message boxes
		"vbs" if windows => argv.extend(["cscript.exe", "//NoLogo"].map(String::from)),
		_ => {}
	}
	argv.push(file.to_string());
	argv.extend(args.iter().cloned());
	argv
}

/// Tells Explorer to read the associations again. Does nothing off Windows.
pub fn changed() {
	#[cfg(windows)]
	{
		use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
		// SAFETY: no item pointers are passed with this event.
		unsafe {
			SHChangeNotify(
				SHCNE_ASSOCCHANGED as i32,
				SHCNF_IDLIST,
				std::ptr::null(),
				std::ptr::null(),
			);
		}
	}
}

#[cfg(windows)]
pub use registry::Registry;

#[cfg(windows)]
mod registry {
	use super::{Store, Value};
	use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
	use windows_sys::Win32::System::Registry::{
		HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE,
		REG_OPTION_NON_VOLATILE, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW,
		RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW, RegSetValueExW,
	};

	#[derive(Debug)]
	pub struct Registry;

	fn wide(s: &str) -> Vec<u16> {
		s.encode_utf16().chain(std::iter::once(0)).collect()
	}

	fn split(key: &str) -> Option<(HKEY, &str)> {
		let (hive, rest) = key.split_once('\\')?;
		match hive {
			"HKCU" => Some((HKEY_CURRENT_USER, rest)),
			"HKCR" => Some((HKEY_CLASSES_ROOT, rest)),
			_ => None,
		}
	}

	// A key handle that closes itself.
	struct Key(HKEY);
	impl Drop for Key {
		fn drop(&mut self) {
			// SAFETY: the handle came from a successful open or create.
			unsafe { RegCloseKey(self.0) };
		}
	}

	fn open(key: &str, access: u32) -> Option<Key> {
		let (hive, path) = split(key)?;
		let path = wide(path);
		let mut handle: HKEY = std::ptr::null_mut();
		// SAFETY: a plain open; the handle is owned by `Key` from here.
		let status = unsafe { RegOpenKeyExW(hive, path.as_ptr(), 0, access, &raw mut handle) };
		(status == ERROR_SUCCESS).then_some(Key(handle))
	}

	fn fail(what: &str, key: &str, status: u32) -> String {
		format!(
			"{what} {key}: {}",
			std::io::Error::from_raw_os_error(status as i32)
		)
	}

	impl Store for Registry {
		fn get(&self, key: &str, name: &str) -> Option<Value> {
			let handle = open(key, KEY_QUERY_VALUE)?;
			let name = wide(name);
			let mut kind = 0u32;
			let mut bytes = 0u32;
			// SAFETY: a size query; no buffer is passed.
			let status = unsafe {
				RegQueryValueExW(
					handle.0,
					name.as_ptr(),
					std::ptr::null(),
					&raw mut kind,
					std::ptr::null_mut(),
					&raw mut bytes,
				)
			};
			if status != ERROR_SUCCESS {
				return None;
			}
			let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
			let mut bytes = (buf.len() * 2) as u32;
			// SAFETY: `bytes` is the buffer's size in bytes.
			let status = unsafe {
				RegQueryValueExW(
					handle.0,
					name.as_ptr(),
					std::ptr::null(),
					&raw mut kind,
					buf.as_mut_ptr().cast::<u8>(),
					&raw mut bytes,
				)
			};
			if status != ERROR_SUCCESS {
				return None;
			}
			let chars = (bytes as usize / 2).min(buf.len());
			let text = String::from_utf16_lossy(&buf[..chars]);
			Some(Value {
				kind,
				text: text.trim_end_matches('\0').to_string(),
			})
		}

		fn set(&mut self, key: &str, name: &str, value: &Value) -> Result<(), String> {
			let Some((hive, path)) = split(key).filter(|(h, _)| *h == HKEY_CURRENT_USER) else {
				return Err(format!("not a per-user key: {key}"));
			};
			let wpath = wide(path);
			let mut handle: HKEY = std::ptr::null_mut();
			// SAFETY: create-or-open under HKCU; the handle is owned by `Key`.
			let status = unsafe {
				RegCreateKeyExW(
					hive,
					wpath.as_ptr(),
					0,
					std::ptr::null(),
					REG_OPTION_NON_VOLATILE,
					KEY_SET_VALUE,
					std::ptr::null(),
					&raw mut handle,
					std::ptr::null_mut(),
				)
			};
			if status != ERROR_SUCCESS {
				return Err(fail("cannot create", key, status));
			}
			let handle = Key(handle);
			let name = wide(name);
			let data: Vec<u16> = if value.kind == super::REG_NONE {
				Vec::new()
			} else {
				wide(&value.text)
			};
			// SAFETY: `data` outlives the call, and the length is in bytes.
			let status = unsafe {
				RegSetValueExW(
					handle.0,
					name.as_ptr(),
					0,
					value.kind,
					data.as_ptr().cast::<u8>(),
					(data.len() * 2) as u32,
				)
			};
			if status == ERROR_SUCCESS {
				Ok(())
			} else {
				Err(fail("cannot write", key, status))
			}
		}

		fn delete(&mut self, key: &str, name: &str) -> Result<(), String> {
			let Some(handle) = open(key, KEY_SET_VALUE) else {
				return Ok(());
			};
			let name = wide(name);
			// SAFETY: an open handle and a terminated name.
			let status = unsafe { RegDeleteValueW(handle.0, name.as_ptr()) };
			if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
				Ok(())
			} else {
				Err(fail("cannot delete from", key, status))
			}
		}

		fn exists(&self, key: &str) -> bool {
			open(key, KEY_READ).is_some()
		}

		fn prune(&mut self, key: &str) -> Result<(), String> {
			let Some(handle) = open(key, KEY_READ) else {
				return Ok(());
			};
			let (mut subkeys, mut values) = (0u32, 0u32);
			// SAFETY: only the two counts are asked for; every other out pointer is null.
			let status = unsafe {
				RegQueryInfoKeyW(
					handle.0,
					std::ptr::null_mut(),
					std::ptr::null_mut(),
					std::ptr::null(),
					&raw mut subkeys,
					std::ptr::null_mut(),
					std::ptr::null_mut(),
					&raw mut values,
					std::ptr::null_mut(),
					std::ptr::null_mut(),
					std::ptr::null_mut(),
					std::ptr::null_mut(),
				)
			};
			drop(handle);
			if status != ERROR_SUCCESS || subkeys > 0 || values > 0 {
				return Ok(());
			}
			let Some((hive, path)) = split(key).filter(|(h, _)| *h == HKEY_CURRENT_USER) else {
				return Ok(());
			};
			let path = wide(path);
			// SAFETY: a terminated path under HKCU, checked empty just above.
			let status = unsafe { RegDeleteKeyW(hive, path.as_ptr()) };
			if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
				Ok(())
			} else {
				Err(fail("cannot remove", key, status))
			}
		}
	}
}

/// A registry in a map, for the tests and for any platform with no registry.
#[cfg(any(test, not(windows)))]
#[derive(Default, Clone, Debug)]
pub struct Memory {
	pub keys: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>,
}

#[cfg(any(test, not(windows)))]
impl Memory {
	fn fold(key: &str) -> String {
		key.to_ascii_lowercase()
	}
}

#[cfg(any(test, not(windows)))]
impl Store for Memory {
	fn get(&self, key: &str, name: &str) -> Option<Value> {
		self.keys
			.get(&Self::fold(key))?
			.get(&name.to_ascii_lowercase())
			.cloned()
	}
	fn set(&mut self, key: &str, name: &str, value: &Value) -> Result<(), String> {
		if !key.starts_with(r"HKCU\") {
			return Err(format!("not a per-user key: {key}"));
		}
		// creating a key creates every key above it, as the registry does
		let mut at = key;
		while let Some(cut) = at.rfind('\\') {
			self.keys.entry(Self::fold(at)).or_default();
			at = &at[..cut];
		}
		self.keys
			.entry(Self::fold(key))
			.or_default()
			.insert(name.to_ascii_lowercase(), value.clone());
		Ok(())
	}
	fn delete(&mut self, key: &str, name: &str) -> Result<(), String> {
		if let Some(values) = self.keys.get_mut(&Self::fold(key)) {
			values.remove(&name.to_ascii_lowercase());
		}
		Ok(())
	}
	fn exists(&self, key: &str) -> bool {
		self.keys.contains_key(&Self::fold(key))
	}
	fn prune(&mut self, key: &str) -> Result<(), String> {
		let folded = Self::fold(key);
		let below = format!("{folded}\\");
		let empty = self
			.keys
			.get(&folded)
			.is_some_and(std::collections::BTreeMap::is_empty)
			&& !self.keys.keys().any(|k| k.starts_with(&below));
		if empty {
			self.keys.remove(&folded);
		}
		Ok(())
	}
}

/// The store the Settings dialog writes through.
pub fn system() -> Box<dyn Store + Send> {
	#[cfg(windows)]
	{
		Box::new(Registry)
	}
	#[cfg(not(windows))]
	{
		Box::new(Memory::default())
	}
}

#[cfg(test)]
mod tests {
	use super::{
		Assoc, Memory, REG_SZ, Store, Value, exe_to_register, open_argv, overridden, register,
		registered, unregister,
	};
	use std::path::{Path, PathBuf};

	const EXE: &str = r"C:\Tools\SilkTerm\silkterm.exe";

	// Enough of a stock Windows for the rules: the type-to-program-ID keys in
	// the merged view, and one user key that was there before us.
	fn stock() -> Memory {
		let mut m = Memory::default();
		for (ext, id) in [
			(".bat", "batfile"),
			(".cmd", "cmdfile"),
			(".ps1", "Microsoft.PowerShellScript.1"),
			(".vbs", "VBSFile"),
		] {
			m.keys
				.entry(format!(r"hkcr\{ext}"))
				.or_default()
				.insert(String::new(), Value::sz(id));
		}
		m.set(r"HKCU\Software\Classes", "", &Value::sz("")).unwrap();
		m.delete(r"HKCU\Software\Classes", "").unwrap();
		m
	}

	fn command(m: &Memory, prog_id: &str) -> Option<String> {
		m.get(
			&format!(r"HKCU\Software\Classes\{prog_id}\shell\open\command"),
			"",
		)
		.map(|v| v.text)
	}

	// Test ID: ErNFYUb
	#[test]
	fn put_back_leaves_the_registry_as_it_was() {
		for assoc in Assoc::ALL {
			let mut m = stock();
			// something of the user's under the same program ID
			m.set(
				r"HKCU\Software\Classes\VBSFile\shell\edit\command",
				"",
				&Value::sz("notepad \"%1\""),
			)
			.unwrap();
			let before = m.keys.clone();
			register(assoc, Path::new(EXE), &mut m).unwrap();
			assert!(registered(assoc, &m), "{assoc:?}");
			assert_ne!(m.keys, before, "{assoc:?} wrote nothing");
			unregister(assoc, &mut m).unwrap();
			assert!(!registered(assoc, &m), "{assoc:?}");
			assert_eq!(m.keys, before, "{assoc:?} left something behind");
		}
	}

	// Test ID: ErNFYUc
	#[test]
	fn a_double_click_runs_silkterm_with_the_file() {
		let mut m = stock();
		register(Assoc::Batch, Path::new(EXE), &mut m).unwrap();
		let want = format!("\"{EXE}\" --keep-open --open \"%1\" %*");
		assert_eq!(command(&m, "batfile").as_deref(), Some(want.as_str()));
		assert_eq!(command(&m, "cmdfile").as_deref(), Some(want.as_str()));
		assert_eq!(command(&m, "SilkTerm.bat").as_deref(), Some(want.as_str()));
		assert!(
			m.get(
				r"HKCU\Software\Classes\.bat\OpenWithProgids",
				"SilkTerm.bat"
			)
			.is_some()
		);
		// a type the user moved to another program ID follows it
		let mut m = stock();
		m.keys
			.get_mut(r"hkcr\.vbs")
			.unwrap()
			.insert(String::new(), Value::sz("MyVbs"));
		register(Assoc::VbScript, Path::new(EXE), &mut m).unwrap();
		assert!(command(&m, "MyVbs").is_some());
		assert!(command(&m, "VBSFile").is_none());
	}

	// Test ID: ErNFYUd
	#[test]
	fn put_back_restores_a_value_that_was_there_and_its_type() {
		let mut m = stock();
		let theirs = Value {
			kind: 2, // REG_EXPAND_SZ
			text: "\"%SystemRoot%\\System32\\WScript.exe\" \"%1\" %*".into(),
		};
		let key = r"HKCU\Software\Classes\VBSFile\shell\open\command";
		m.set(key, "", &theirs).unwrap();
		register(Assoc::VbScript, Path::new(EXE), &mut m).unwrap();
		assert_eq!(m.get(key, "").unwrap().kind, REG_SZ);
		unregister(Assoc::VbScript, &mut m).unwrap();
		assert_eq!(m.get(key, ""), Some(theirs));
	}

	// Test ID: ErNFYUe
	#[test]
	fn put_back_leaves_a_value_somebody_changed_since() {
		let mut m = stock();
		register(Assoc::Batch, Path::new(EXE), &mut m).unwrap();
		let key = r"HKCU\Software\Classes\batfile\shell\open\command";
		m.set(key, "", &Value::sz("other.exe \"%1\"")).unwrap();
		unregister(Assoc::Batch, &mut m).unwrap();
		assert_eq!(
			m.get(key, "").map(|v| v.text).as_deref(),
			Some("other.exe \"%1\"")
		);
		assert!(
			command(&m, "cmdfile").is_none(),
			"the untouched one still goes back"
		);
	}

	// Test ID: ErNFYUf
	#[test]
	fn registering_again_keeps_what_was_there_first() {
		let mut m = stock();
		let key = r"HKCU\Software\Classes\batfile\shell\open\command";
		m.set(key, "", &Value::sz("first.exe")).unwrap();
		register(Assoc::Batch, Path::new(EXE), &mut m).unwrap();
		let moved = r"D:\Elsewhere\silkterm.exe";
		register(Assoc::Batch, Path::new(moved), &mut m).unwrap();
		assert!(command(&m, "batfile").unwrap().contains(moved));
		unregister(Assoc::Batch, &mut m).unwrap();
		assert_eq!(m.get(key, "").map(|v| v.text).as_deref(), Some("first.exe"));
	}

	// Test ID: ErNFYUg
	#[test]
	fn the_folder_entry_opens_the_folder() {
		let mut m = stock();
		register(Assoc::Folder, Path::new(EXE), &mut m).unwrap();
		for place in [
			r"Directory\shell",
			r"Directory\Background\shell",
			r"Drive\shell",
		] {
			let verb = format!(r"HKCU\Software\Classes\{place}\SilkTerm");
			assert_eq!(
				m.get(&verb, "").map(|v| v.text).as_deref(),
				Some("Open in SilkTerm")
			);
			assert_eq!(
				m.get(&format!(r"{verb}\command"), "").map(|v| v.text),
				Some(format!("\"{EXE}\" --directory \"%V\""))
			);
		}
	}

	// Test ID: ErNFYUh
	#[test]
	fn a_type_the_user_picked_an_app_for_is_reported() {
		let mut m = stock();
		let choice =
			r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.ps1\UserChoice";
		assert!(overridden(Assoc::PowerShell, &m).is_empty());
		m.set(
			choice,
			"ProgId",
			&Value::sz("AppXxf01pj590w7z9mxmyv3nx0a9ewj3e51g"),
		)
		.unwrap();
		assert_eq!(overridden(Assoc::PowerShell, &m), [".ps1"]);
		m.set(choice, "ProgId", &Value::sz("SilkTerm.ps1")).unwrap();
		assert!(overridden(Assoc::PowerShell, &m).is_empty());
		m.set(choice, "ProgId", &Value::sz("microsoft.powershellscript.1"))
			.unwrap();
		assert!(overridden(Assoc::PowerShell, &m).is_empty());
	}

	// Test ID: ErNFYUi
	#[test]
	fn a_dogfood_build_registers_the_link_beside_its_versions_folder() {
		let versioned = PathBuf::from("/l/Programs/silkterm_versions/silkterm_newest.exe");
		let link = PathBuf::from("/l/Programs/silkterm.exe");
		assert_eq!(exe_to_register(&versioned, &|p| p == link), link);
		assert_eq!(exe_to_register(&versioned, &|_| false), versioned);
		let installed = PathBuf::from("/p/SilkTerm/silkterm.exe");
		assert_eq!(exe_to_register(&installed, &|_| true), installed);
	}

	// Test ID: ErNFYUj
	#[test]
	fn open_picks_the_host_by_file_type() {
		let args = vec!["a b".to_string()];
		assert_eq!(
			open_argv(r"C:\x y\go.bat", &args, true, &|| false),
			[r"C:\x y\go.bat", "a b"]
		);
		assert_eq!(
			open_argv(r"C:\x\run.VBS", &args, true, &|| false),
			["cscript.exe", "//NoLogo", r"C:\x\run.VBS", "a b"]
		);
		let ps = open_argv(r"C:\it's\s.ps1", &args, true, &|| true);
		assert_eq!(ps[..3], ["pwsh.exe", "-NoLogo", "-Command"]);
		assert!(ps[3].ends_with(r"& 'C:\it''s\s.ps1' 'a b'"), "{}", ps[3]);
		assert!(ps[3].contains("-ne 'AllSigned'"));
		assert_eq!(
			open_argv("x.ps1", &[], true, &|| false)[0],
			"powershell.exe"
		);
		// elsewhere a file is its own program
		assert_eq!(
			open_argv("/tmp/x.ps1", &[], false, &|| true),
			["/tmp/x.ps1"]
		);
	}

	// The real registry, on a key of its own that it removes again. Types go
	// through unchanged, an empty REG_NONE marker included, and prune takes only
	// an empty key.
	// Test ID: ErNHwGL
	#[cfg(windows)]
	#[test]
	fn the_registry_store_round_trips() {
		let mut reg = super::Registry;
		let root = format!(r"HKCU\Software\SilkTerm-test-{}", std::process::id());
		let key = format!(r"{root}\a\b");
		let expand = Value {
			kind: 2,
			text: r"%SystemRoot%\x.exe".into(),
		};
		let marker = Value {
			kind: super::REG_NONE,
			text: String::new(),
		};
		reg.set(&key, "", &Value::sz("one")).unwrap();
		reg.set(&key, "e", &expand).unwrap();
		reg.set(&key, "m", &marker).unwrap();
		assert_eq!(reg.get(&key, "").map(|v| v.text).as_deref(), Some("one"));
		assert_eq!(reg.get(&key, "e"), Some(expand));
		assert_eq!(reg.get(&key, "m"), Some(marker));
		reg.prune(&key).unwrap();
		assert!(reg.exists(&key), "a key with values stays");
		for name in ["", "e", "m"] {
			reg.delete(&key, name).unwrap();
		}
		reg.delete(&key, "gone").unwrap();
		for at in [key.as_str(), &format!(r"{root}\a"), &root] {
			reg.prune(at).unwrap();
		}
		assert!(!reg.exists(&root));
		assert!(
			reg.set(r"HKCR\.silkterm-test", "", &Value::sz("x"))
				.is_err()
		);
		// the merged view answers for a type every Windows has
		assert!(reg.get(r"HKCR\.bat", "").is_some());
	}
}
