#include "runtime-test.capnp.h"
#include <capnp/dynamic.h>
#include <capnp/message.h>
#include <capnp/orphan.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <optional>
#include <sstream>

struct Echo final: Harness::Server {
  Echo(unsigned value,unsigned& live): value(value),live(live) {++live;}
  ~Echo() {--live;}
  unsigned value; unsigned& live;
  kj::Promise<void> echo(EchoContext c) override {c.getResults().setValue(value);return kj::READY_NOW;}
};
unsigned value(Harness::Client cap,kj::WaitScope& wait) {
  return cap.echoRequest().send().wait(wait).getValue();
}
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);
  kj::EventLoop loop; kj::WaitScope wait(loop);
  std::ifstream input(argv[1]); KJ_REQUIRE(input.good());
  std::string line;
  while (std::getline(input,line)) {
    std::istringstream events(line);
    unsigned words,live=0; events>>words;
    capnp::MallocMessageBuilder message(words),foreign;
    auto root=message.getRoot<capnp::AnyPointer>();
    auto wrong=foreign.getRoot<capnp::AnyPointer>();
    auto token=capnp::Orphanage::getForMessageContaining(root);
    std::optional<capnp::Orphan<Harness::PendingResults>> orphan;
    unsigned event;
    while (events>>event) {
      switch (event) {
        case 1: case 2:
          orphan.emplace(token.newOrphan<Harness::PendingResults>());
          orphan->get().setCap(kj::heap<Echo>(event,live)); break;
        case 3: root.adopt(kj::mv(*orphan)); orphan.reset(); break;
        case 4: orphan.emplace(root.disownAs<Harness::PendingResults>()); break;
        case 5: orphan.reset(); break;
        case 6: root.clear(); break;
        case 7: {
          bool rejected=false;
          try {wrong.adopt(kj::mv(*orphan));} catch (kj::Exception&) {rejected=true;}
          KJ_REQUIRE(rejected && wrong.isNull()); break;
        }
        case 8: {
          auto dynamic=capnp::Orphan<capnp::DynamicStruct>(kj::mv(*orphan));
          bool rejected=false;
          try {auto bad=dynamic.releaseAs<Harness::Value>();} catch (kj::Exception&) {rejected=true;}
          KJ_REQUIRE(rejected);
          orphan.emplace(dynamic.releaseAs<Harness::PendingResults>()); break;
        }
        case 9: orphan.emplace(token.newOrphanCopy(root.getAs<Harness::PendingResults>().asReader())); break;
        case 10: orphan.emplace(); break;
        default: KJ_FAIL_REQUIRE("unknown event",event);
      }
      unsigned r=root.isNull()?0:value(root.getAs<Harness::PendingResults>().getCap(),wait);
      unsigned o=!orphan?0:(*orphan==nullptr?3:value(orphan->get().getCap(),wait));
      std::cout<<r<<' '<<o<<' '<<live<<'\n';
    }
    orphan.reset();root.clear();KJ_REQUIRE(live==0);
  }
}
