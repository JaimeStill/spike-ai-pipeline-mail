/*
 * rusage runs one program and reports what the kernel measured of it: its peak resident memory
 * (ru_maxrss), its user and system CPU time, and its wall time, with a timeout. The findings run
 * (scripts/findings.sh) times the CLI through it, because a child's ru_maxrss carries its
 * parent's resident memory across the fork and exec that start it: started from the findings
 * program, the CLI would never report less than that program's own peak. Started from this small
 * process, the CLI's peak is its own, above a floor of this program's resident memory, which the
 * findings run measures by running /bin/true through it.
 *
 * Copied from spike-ai-pipeline-office's bench/rusage.c (Apache-2.0, the same author), at
 * 6ed0d0e; only this comment differs.
 *
 * Usage: rusage <timeout-seconds> <stdout-file> <stderr-file> <program> [args...]
 *
 * The program's stdout and stderr go to the two files. On success rusage prints one line,
 *   status=<exited|signaled|timeout> code=<n> maxrss_kib=<n> wall_ns=<n> user_us=<n> sys_us=<n>
 * and exits 0, whatever the program's own exit; it exits 2 on a usage error and 1 when it
 * couldn't start or wait for the program. A program past its timeout is killed, with its
 * process group, and reported as timeout.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static long long ns_between(const struct timespec *a, const struct timespec *b)
{
	return (long long)(b->tv_sec - a->tv_sec) * 1000000000LL + (b->tv_nsec - a->tv_nsec);
}

static long long us_of(const struct timeval *t)
{
	return (long long)t->tv_sec * 1000000LL + t->tv_usec;
}

int main(int argc, char **argv)
{
	if (argc < 5) {
		fprintf(stderr, "usage: rusage <timeout-seconds> <stdout-file> <stderr-file> <program> [args...]\n");
		return 2;
	}
	char *end;
	long timeout = strtol(argv[1], &end, 10);
	if (*end != '\0' || timeout <= 0) {
		fprintf(stderr, "rusage: timeout must be a positive number of seconds: %s\n", argv[1]);
		return 2;
	}

	/* SIGCHLD is blocked before the fork, so sigtimedwait can wait for it with the timeout. */
	sigset_t chld;
	sigemptyset(&chld);
	sigaddset(&chld, SIGCHLD);
	if (sigprocmask(SIG_BLOCK, &chld, NULL) != 0) {
		perror("rusage: sigprocmask");
		return 1;
	}

	struct timespec start, stop;
	clock_gettime(CLOCK_MONOTONIC, &start);
	pid_t pid = fork();
	if (pid < 0) {
		perror("rusage: fork");
		return 1;
	}
	if (pid == 0) {
		/* Its own process group, so a timeout kills whatever it started too. */
		setpgid(0, 0);
		sigprocmask(SIG_UNBLOCK, &chld, NULL);
		int out = open(argv[2], O_WRONLY | O_CREAT | O_TRUNC, 0644);
		int err = open(argv[3], O_WRONLY | O_CREAT | O_TRUNC, 0644);
		int in = open("/dev/null", O_RDONLY);
		if (out < 0 || err < 0 || in < 0 || dup2(out, 1) < 0 || dup2(err, 2) < 0 || dup2(in, 0) < 0) {
			_exit(127);
		}
		execv(argv[4], &argv[4]);
		_exit(127);
	}

	struct timespec wait = {.tv_sec = timeout, .tv_nsec = 0};
	int timedout = 0;
	for (;;) {
		struct timespec left;
		clock_gettime(CLOCK_MONOTONIC, &left);
		long long remaining = (long long)timeout * 1000000000LL - ns_between(&start, &left);
		if (remaining <= 0) {
			timedout = 1;
			break;
		}
		wait.tv_sec = remaining / 1000000000LL;
		wait.tv_nsec = remaining % 1000000000LL;
		int sig = sigtimedwait(&chld, NULL, &wait);
		if (sig == SIGCHLD) {
			break;
		}
		if (sig < 0 && errno == EAGAIN) {
			timedout = 1;
			break;
		}
		if (sig < 0 && errno != EINTR) {
			perror("rusage: sigtimedwait");
			return 1;
		}
	}
	if (timedout) {
		kill(-pid, SIGKILL);
		kill(pid, SIGKILL);
	}

	int status;
	struct rusage usage;
	if (wait4(pid, &status, 0, &usage) != pid) {
		perror("rusage: wait4");
		return 1;
	}
	clock_gettime(CLOCK_MONOTONIC, &stop);

	const char *kind = timedout ? "timeout" : WIFEXITED(status) ? "exited" : "signaled";
	int code = WIFEXITED(status) ? WEXITSTATUS(status) : WIFSIGNALED(status) ? WTERMSIG(status) : -1;
	printf("status=%s code=%d maxrss_kib=%ld wall_ns=%lld user_us=%lld sys_us=%lld\n", kind, code,
	       usage.ru_maxrss, ns_between(&start, &stop), us_of(&usage.ru_utime), us_of(&usage.ru_stime));
	return 0;
}
