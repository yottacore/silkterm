// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// The soak test's planted leak, preloaded with run.bash --plant: every child the
// program starts (a tab's or a pane's shell, a new window) leaks 64 KiB in the
// parent. The PTY itself is opened by raw syscalls, so the spawn is the hook.

#define _GNU_SOURCE
#include <dlfcn.h>
#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void plant(void) {
	char *volatile p = malloc(64 * 1024);
	if (p) memset(p, 0x5a, 64 * 1024);
}

pid_t fork(void) {
	static pid_t (*real)(void);
	if (!real) real = (pid_t (*)(void))dlsym(RTLD_NEXT, "fork");
	pid_t pid = real();
	if (pid > 0) plant();
	return pid;
}

typedef int (*spawn_fn)(pid_t *, const char *, const posix_spawn_file_actions_t *, const posix_spawnattr_t *,
	char *const[], char *const[]);

static int spawned(const char *name, pid_t *pid, const char *path, const posix_spawn_file_actions_t *fa,
	const posix_spawnattr_t *attr, char *const argv[], char *const envp[]) {
	spawn_fn real = (spawn_fn)dlsym(RTLD_NEXT, name);
	int rc = real(pid, path, fa, attr, argv, envp);
	if (rc == 0) plant();
	return rc;
}

int posix_spawn(pid_t *pid, const char *path, const posix_spawn_file_actions_t *fa, const posix_spawnattr_t *attr,
	char *const argv[], char *const envp[]) {
	return spawned("posix_spawn", pid, path, fa, attr, argv, envp);
}

int posix_spawnp(pid_t *pid, const char *file, const posix_spawn_file_actions_t *fa, const posix_spawnattr_t *attr,
	char *const argv[], char *const envp[]) {
	return spawned("posix_spawnp", pid, file, fa, attr, argv, envp);
}
