#include "runtime-test.capnp.h"
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <kj/io.h>
#include <fcntl.h>
#include <unistd.h>
#include <fstream>
#include <iostream>
#include <memory>
#include <optional>
#include <sstream>

unsigned value(int fd) {
  if (fd < 0) return 0;
  unsigned char byte=0;
  KJ_REQUIRE(pread(fd, &byte, 1, 0)==1);
  return byte;
}
struct FileCap final: Harness::Server {
  unsigned number;
  unsigned& calls;
  kj::AutoCloseFd fd;
  FileCap(unsigned number, unsigned& calls): number(number), calls(calls) {
    char path[]="/tmp/reproto-facade-fd-XXXXXX";
    int raw=mkstemp(path);
    KJ_REQUIRE(raw>=0);
    fd=kj::AutoCloseFd(raw);
    KJ_REQUIRE(unlink(path)==0);
    unsigned char byte=number;
    KJ_REQUIRE(write(raw,&byte,1)==1);
  }
  kj::Maybe<int> getFd() override { return fd.get(); }
  kj::Promise<void> echo(EchoContext c) override {
    ++calls;
    c.getResults().setValue(number);
    return kj::READY_NOW;
  }
};
struct Service final: Harness::Server {
  unsigned& received;
  unsigned& calls;
  Service(unsigned& received, unsigned& calls): received(received), calls(calls) {}
  kj::Promise<void> fdCaps(FdCapsContext context) override {
    auto caps=context.getParams().getCaps();
    KJ_REQUIRE(caps.size()==2);
    received=0;
    auto tasks=kj::heapArrayBuilder<kj::Promise<void>>(2);
    for (unsigned i=0;i<2;++i) {
      auto cap=caps[i];
      tasks.add(cap.getFd().then([this,i,cap=kj::mv(cap)](kj::Maybe<int> fd) mutable {
        KJ_IF_SOME(raw,fd) { received += value(raw)*(i==0?1:10); }
        return cap.echoRequest().send().then([i](auto response) { KJ_REQUIRE(response.getValue()==i+1); });
      }));
    }
    return kj::joinPromises(tasks.finish()).then([this,context=kj::mv(context)]() mutable {
      auto caps=context.getResults().initCaps(2);
      caps.set(0,kj::heap<FileCap>(3,calls));
      caps.set(1,kj::heap<FileCap>(4,calls));
    });
  }
};
// Feed a real capability socket through the C++ descriptor-aware listener path.
struct Receiver final: kj::ConnectionReceiver {
  kj::Own<kj::AsyncIoStream> socket;
  explicit Receiver(kj::Own<kj::AsyncCapabilityStream> socket): socket(kj::mv(socket)) {}
  kj::Promise<kj::Own<kj::AsyncIoStream>> accept() override {
    if (socket.get()!=nullptr) return kj::mv(socket);
    return kj::NEVER_DONE;
  }
  uint getPort() override { return 0; }
};
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);
  std::ifstream input(argv[1]);
  KJ_REQUIRE(input.good());
  auto io=kj::setupAsyncIo();
  auto& wait=io.waitScope;
  std::string line;
  while (std::getline(input,line)) {
    unsigned received=0,calls=0,drained=0;
    std::unique_ptr<capnp::TwoPartyServer> server;
    std::unique_ptr<Receiver> receiver;
    std::optional<kj::Promise<void>> listening;
    kj::Own<kj::AsyncCapabilityStream> serverIo,peerIo;
    std::unique_ptr<capnp::TwoPartyClient> peer;
    std::optional<Harness::Client> remote;
    std::optional<Harness::Client> caps[2];
    kj::AutoCloseFd escaped[2];
    std::optional<kj::Promise<void>> borrowed,drain;
    int rawSocket=-1;
    std::istringstream events(line);
    unsigned event;
    while (events>>event) {
      if (event>=10 && event<=27) {
        unsigned mode=(event-10)/9,slimit=(event-10)%9/3,climit=(event-10)%3;
        server=std::make_unique<capnp::TwoPartyServer>(kj::heap<Service>(received,calls));
        auto pipe=io.provider->newCapabilityPipe();
        serverIo=kj::mv(pipe.ends[0]); peerIo=kj::mv(pipe.ends[1]);
        rawSocket=KJ_REQUIRE_NONNULL(serverIo->getFd());
        if (mode==0) {
          receiver=std::make_unique<Receiver>(kj::mv(serverIo));
          listening.emplace(server->listenCapStreamReceiver(*receiver,slimit).eagerlyEvaluate(nullptr));
        }
        else borrowed.emplace(server->accept(*serverIo,slimit));
        peer=std::make_unique<capnp::TwoPartyClient>(*peerIo,climit);
        remote.emplace(peer->bootstrap().castAs<Harness>());
      } else switch(event) {
        case 1: {
          auto request=remote->fdCapsRequest();
          auto parameters=request.initCaps(2);
          parameters.set(0,kj::heap<FileCap>(1,calls));
          parameters.set(1,kj::heap<FileCap>(2,calls));
          auto response=request.send().wait(wait);
          for (unsigned i=0;i<2;++i) {
            caps[i].emplace(response.getCaps()[i]);
            escaped[i]=kj::AutoCloseFd();
            auto fd=caps[i]->getFd().wait(wait);
            KJ_IF_SOME(raw,fd) {
              int copy=fcntl(raw,F_DUPFD_CLOEXEC,0);
              KJ_REQUIRE(copy>=0);
              escaped[i]=kj::AutoCloseFd(copy);
            }
            KJ_REQUIRE(caps[i]->echoRequest().send().wait(wait).getValue()==i+3);
          }
          break;
        }
        case 2: drain.emplace(server->drain()); drained=1; break;
        case 3:
        case 6:
          if (event==6) borrowed.reset();
          peer.reset(); peerIo=nullptr;
          break;
        case 4:
          for (unsigned i=0;i<2;++i) {
            bool failed=false;
            try { caps[i]->echoRequest().send().wait(wait); }
            catch (const kj::Exception&) { failed=true; }
            KJ_REQUIRE(failed);
          }
          break;
        default: KJ_FAIL_REQUIRE("unexpected event",event);
      }
      wait.poll();
      if (borrowed && borrowed->poll(wait)) { borrowed->wait(wait); borrowed.reset(); }
      if (drain && drain->poll(wait)) { drain->wait(wait); drain.reset(); drained=2; }
      std::cout<<received<<' '<<value(escaped[0].get())+10*value(escaped[1].get())<<' '
               <<calls<<' '<<drained<<' '<<unsigned(fcntl(rawSocket,F_GETFD)>=0)<<',';
    }
    std::cout<<'\n';
  }
}
