#include <stdint.h>
#include <errno.h>
#include <time.h>
#ifdef CAPNTPROTO_ALLOCATION_TRACE
#include <valgrind/dhat.h>
void capntproto_record_allocation(uint64_t bytes) { DHAT_AD_HOC_EVENT(bytes); }
#endif
#if defined(__has_include)
#if __has_include(<valgrind/callgrind.h>)
#include <valgrind/callgrind.h>
#define HAVE_CALLGRIND 1
#endif
#endif
#ifndef HAVE_CALLGRIND
#define CALLGRIND_START_INSTRUMENTATION ((void)0)
#define CALLGRIND_STOP_INSTRUMENTATION ((void)0)
#define CALLGRIND_ZERO_STATS ((void)0)
#define CALLGRIND_DUMP_STATS ((void)0)
#endif
#ifdef __linux__
#include <linux/perf_event.h>
#include <sys/syscall.h>
#include <sys/ioctl.h>
#include <unistd.h>
#endif
int capntproto_callgrind(void) {
#ifdef HAVE_CALLGRIND
    return 1;
#else
    return 0;
#endif
}
void capntproto_measure_start(void) { CALLGRIND_START_INSTRUMENTATION; CALLGRIND_ZERO_STATS; }
void capntproto_measure_stop(void) { CALLGRIND_DUMP_STATS; CALLGRIND_STOP_INSTRUMENTATION; }
uint64_t capntproto_cpu_ns(void) {
    struct timespec time;
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID,&time)) return 0;
    return (uint64_t)time.tv_sec*1000000000+(uint64_t)time.tv_nsec;
}
int capntproto_cycles_open(void) {
#ifdef __linux__
    struct perf_event_attr attr={0};
    attr.type=PERF_TYPE_HARDWARE; attr.size=sizeof(attr);
    attr.config=PERF_COUNT_HW_CPU_CYCLES; attr.disabled=1;
    attr.exclude_kernel=1; attr.exclude_hv=1;
    return syscall(__NR_perf_event_open,&attr,0,-1,-1,0);
#else
    errno=ENOTSUP;return -1;
#endif
}
void capntproto_cycles_start(int fd) {
#ifdef __linux__
    if(fd>=0){ioctl(fd,PERF_EVENT_IOC_RESET,0);ioctl(fd,PERF_EVENT_IOC_ENABLE,0);}
#endif
}
uint64_t capntproto_cycles_stop(int fd) {
    uint64_t value=0;
#ifdef __linux__
    if(fd>=0){ioctl(fd,PERF_EVENT_IOC_DISABLE,0);if(read(fd,&value,sizeof(value))!=sizeof(value))value=0;close(fd);}
#endif
    return value;
}
