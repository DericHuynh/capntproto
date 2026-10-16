#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

// Linux single-thread benchmark processes only; the recorded ABI is little-endian.
#if __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error "socket timing requires little-endian event encoding"
#endif

// Diagnostic only: no payload capture and no log writes on the measured path.
// The fixed journal records wall and thread CPU syscall entry/exit timestamps.
// CLOCK_THREAD_CPUTIME_ID adds two clock syscalls per operation; these results
// are paired diagnostics, never performance qualification. Thread IDs prevent
// pairing CPU times across unrelated workers.
enum { READ=1, WRITE=2, READV=3, WRITEV=4, RECV=5, SEND=6, RECVFROM=7,
       SENDTO=8, RECVMSG=9, SENDMSG=10, EPOLL=11, EPOLL_PWAIT=12 };
struct event { uint64_t start, end, cpu_start, cpu_end; int64_t result; int32_t fd; uint32_t op, tid, reserved; };
_Static_assert(sizeof(struct event) == 56, "socket timing event ABI");
static _Thread_local uint32_t thread_id;
static uint64_t cpu_now(void) {
  struct timespec t; clock_gettime(CLOCK_THREAD_CPUTIME_ID,&t);
  return (uint64_t)t.tv_sec*1000000000u+t.tv_nsec;
}
static struct event events[131072];
static _Atomic unsigned count;
static _Atomic int dumped;
static _Thread_local signed char sockets[4096];
static const char *destination;
static uint64_t now(void) {
  struct timespec t; clock_gettime(CLOCK_MONOTONIC_RAW,&t);
  return (uint64_t)t.tv_sec*1000000000u+t.tv_nsec;
}
static int network_fd(int fd) {
  if(fd<0 || fd>=4096) return 0;
  if(!sockets[fd]) {
    struct sockaddr_storage addr; socklen_t length=sizeof(addr);
    sockets[fd]=getpeername(fd,(struct sockaddr*)&addr,&length)==0 &&
      (addr.ss_family==AF_INET || addr.ss_family==AF_INET6) ? 1 : -1;
  }
  return sockets[fd]==1;
}
static void record(uint32_t op,int fd,uint64_t start,uint64_t end,uint64_t cpu_start,uint64_t cpu_end,int64_t result) {
  if(!destination || (op<EPOLL && !network_fd(fd))) return;
  unsigned i=atomic_fetch_add_explicit(&count,1,memory_order_relaxed);
  if(!thread_id) thread_id=(uint32_t)syscall(SYS_gettid);
  if(i<131072) events[i]=(struct event){start,end,cpu_start,cpu_end,result,fd,op,thread_id,0};
}
static void dump(void) {
  if(!destination || atomic_exchange(&dumped,1)) return;
  int fd=syscall(SYS_openat,AT_FDCWD,destination,O_CREAT|O_WRONLY|O_EXCL,0600);
  if(fd<0)return;
  uint64_t n=atomic_load(&count);
  // Counts beyond capacity are retained in the header so analysis rejects them.
  syscall(SYS_write,fd,&n,sizeof(n));
  size_t bytes=(n<131072?n:131072)*sizeof(struct event),offset=0;
  while(offset<bytes) {
    ssize_t wrote=syscall(SYS_write,fd,(char*)events+offset,bytes-offset);
    if(wrote<=0)break;
    offset+=(size_t)wrote;
  }
  syscall(SYS_close,fd);
}
static void terminated(int sig) {dump();_exit(128+sig);}
__attribute__((constructor)) static void init(void) {
  destination=getenv("CAPNTPROTO_IO_TRACE");
  if(destination){atexit(dump);signal(SIGTERM,terminated);}
}
#define WRAP(name,type,args,call,operation) \
type name args { \
  static type (*original) args; \
  if(!original) original=dlsym(RTLD_NEXT,#name); \
  uint64_t start=destination?now():0,cpu_start=destination?cpu_now():0; \
  type result=original call; int saved_errno=errno; \
  if(destination) { uint64_t cpu_end=cpu_now(); record(operation,fd,start,now(),cpu_start,cpu_end,result); } \
  errno=saved_errno;return result; \
}
WRAP(read,ssize_t,(int fd,void *b,size_t n),(fd,b,n),READ)
WRAP(write,ssize_t,(int fd,const void *b,size_t n),(fd,b,n),WRITE)
WRAP(readv,ssize_t,(int fd,const struct iovec *v,int n),(fd,v,n),READV)
WRAP(writev,ssize_t,(int fd,const struct iovec *v,int n),(fd,v,n),WRITEV)
WRAP(recv,ssize_t,(int fd,void *b,size_t n,int flags),(fd,b,n,flags),RECV)
WRAP(send,ssize_t,(int fd,const void *b,size_t n,int flags),(fd,b,n,flags),SEND)
WRAP(recvfrom,ssize_t,(int fd,void *b,size_t n,int flags,struct sockaddr *addr,socklen_t *len),(fd,b,n,flags,addr,len),RECVFROM)
WRAP(sendto,ssize_t,(int fd,const void *b,size_t n,int flags,const struct sockaddr *addr,socklen_t len),(fd,b,n,flags,addr,len),SENDTO)
WRAP(recvmsg,ssize_t,(int fd,struct msghdr *msg,int flags),(fd,msg,flags),RECVMSG)
WRAP(sendmsg,ssize_t,(int fd,const struct msghdr *msg,int flags),(fd,msg,flags),SENDMSG)
WRAP(epoll_wait,int,(int fd,struct epoll_event *ev,int n,int timeout),(fd,ev,n,timeout),EPOLL)
WRAP(epoll_pwait,int,(int fd,struct epoll_event *ev,int n,int timeout,const sigset_t *mask),(fd,ev,n,timeout,mask),EPOLL_PWAIT)
int close(int fd) {
  if(fd>=0 && fd<4096)sockets[fd]=0;
  return syscall(SYS_close,fd);
}
