/* spawnbench.c — fork+execve vs posix_spawn cost on this host.
 * Build: cc -O2 -o /tmp/spawnbench scripts/perf/spawnbench.c
 * Run:   /tmp/spawnbench [parent-heap-MB]
 * Recorded 2026-09-16 (macOS, arm64): fork+exec 1.13 ms, posix_spawn 0.77 ms. */
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
extern char **environ;
static double now(){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
int main(int argc,char**argv){
    int n=1000; char*args[]={"/usr/bin/true",NULL}; int st;
    /* inflate heap to mimic a bigger parent */
    size_t mb = argc>1?atoi(argv[1]):0; char*p=malloc(mb<<20); if(p) memset(p,1,mb<<20);
    double t0=now();
    for(int i=0;i<n;i++){pid_t c=fork(); if(c==0){execv(args[0],args);_exit(127);} waitpid(c,&st,0);}
    double t1=now();
    for(int i=0;i<n;i++){pid_t c; posix_spawn(&c,args[0],NULL,NULL,args,environ); waitpid(c,&st,0);}
    double t2=now();
    printf("heap %zuMB: fork+exec %.3f ms/iter, posix_spawn %.3f ms/iter\n",mb,(t1-t0)*1000/n,(t2-t1)*1000/n);
    return 0;
}
