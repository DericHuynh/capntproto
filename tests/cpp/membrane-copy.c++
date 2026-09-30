#include "membrane-copy.capnp.h"
#include <capnp/membrane.h>
#include <capnp/message.h>
#include <capnp/orphan.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <optional>
#include <sstream>
#include <vector>
using namespace membraneCopy;
struct Stats {unsigned live=0,calls=0,direction=0;};
struct Server final:Service::Server {
  Stats& stats;
  explicit Server(Stats& stats):stats(stats){++stats.live;}
  ~Server(){--stats.live;}
  kj::Promise<void> ping(PingContext c) override {
    ++stats.calls;c.getResults().setValue(17);return kj::READY_NOW;
  }
};
struct Boundary final:capnp::MembranePolicy,kj::Refcounted {
  Stats& stats;kj::ForkedPromise<void> signal;
  Boundary(Stats& stats,kj::Promise<void> promise):stats(stats),signal(promise.fork()){}
  kj::Own<capnp::MembranePolicy> addRef() override {return kj::addRef(*this);}
  kj::Maybe<kj::Promise<void>> onRevoked() override {return signal.addBranch();}
  kj::Maybe<capnp::Capability::Client> inboundCall(uint64_t,uint16_t,capnp::Capability::Client) override {stats.direction=1;return kj::none;}
  kj::Maybe<capnp::Capability::Client> outboundCall(uint64_t,uint16_t,capnp::Capability::Client) override {stats.direction=2;return kj::none;}
};
const void* identity(Service::Client& cap) {return capnp::ClientHook::from(Service::Client(cap)).get();}
void fill(Payload::Builder value,Service::Client& cap) {
  value.setNumber(73);value.setCap(cap);value.setOther(cap);
  auto caps=value.initCaps(2);caps.set(0,cap);caps.set(1,cap);
  auto nested=value.initNested(1)[0];nested.setNumber(91);nested.setCap(cap);
  value.initOpaque().setAs<Service>(cap);
}
void fill(unsigned form,capnp::AnyPointer::Builder root,Service::Client& cap) {
  if(form==0 || form==2) fill(root.initAs<Payload>(),cap);
  else if(form==1) for(auto value:root.initAs<capnp::List<Payload>>(2)) fill(value,cap);
  else {auto list=root.initAs<capnp::List<Service>>(2);list.set(0,cap);list.set(1,cap);}
}
void append(Payload::Reader value,std::vector<Service::Client>& caps) {
  KJ_REQUIRE(value.getNumber()==73);caps.push_back(value.getCap());caps.push_back(value.getOther());
  for(auto cap:value.getCaps()) caps.push_back(kj::mv(cap));
  auto child=value.getNested()[0];KJ_REQUIRE(child.getNumber()==91);caps.push_back(child.getCap());
  caps.push_back(value.getOpaque().getAs<Service>());
}
std::vector<Service::Client> read(unsigned form,capnp::AnyPointer::Reader root) {
  std::vector<Service::Client> caps;
  if(form==0 || form==2) append(root.getAs<Payload>(),caps);
  else if(form==1) for(auto value:root.getAs<capnp::List<Payload>>()) append(value,caps);
  else for(auto cap:root.getAs<capnp::List<Service>>()) caps.push_back(kj::mv(cap));
  return caps;
}
std::vector<Service::Client> read(unsigned form,capnp::Orphan<capnp::AnyPointer>& orphan) {
  std::vector<Service::Client> caps;
  if(form==0 || form==2) append(orphan.getAsReader<Payload>(),caps);
  else if(form==1) for(auto value:orphan.getAsReader<capnp::List<Payload>>()) append(value,caps);
  else for(auto cap:orphan.getAsReader<capnp::List<Service>>()) caps.push_back(kj::mv(cap));
  return caps;
}
unsigned observe(std::vector<Service::Client> caps,Stats& stats,const void* original,kj::WaitScope& wait) {
  if(caps.empty())return 0;
  auto first=identity(caps[0]);stats.direction=0;
  try {
    KJ_REQUIRE(caps[0].pingRequest().send().wait(wait).getValue()==17);
    for(auto& cap:caps)KJ_REQUIRE(identity(cap)==first);
    if(first==original){KJ_REQUIRE(stats.direction==0);return 3;}
    KJ_REQUIRE(stats.direction==1 || stats.direction==2);return stats.direction;
  }catch(kj::Exception& e){KJ_REQUIRE(e.getDescription().contains("revoked"),e);return 4;}
}
capnp::Orphan<capnp::AnyPointer> copy(unsigned form,bool inward,capnp::AnyPointer::Reader from,
    capnp::Orphanage to,Boundary& policy) {
  auto apply=[&](auto value)->capnp::Orphan<capnp::AnyPointer>{
    if(inward)return capnp::copyIntoMembrane(value,to,policy.addRef());
    return capnp::copyOutOfMembrane(value,to,policy.addRef());
  };
  switch(form){
    case 0:return apply(from.getAs<Payload>());
    case 1:return apply(from.getAs<capnp::List<Payload>>());
    case 2:return apply(from);
    case 3:return apply(from.getAs<capnp::List<Service>>());
    default:KJ_FAIL_REQUIRE("bad form");
  }
}
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);std::ifstream input(argv[1]);KJ_REQUIRE(input.good());
  kj::EventLoop loop;kj::WaitScope wait(loop);std::string line;
  while(std::getline(input,line)) {
    std::istringstream events(line);unsigned form,event;events>>form;
    Stats stats;Service::Client cap(kj::heap<Server>(stats));auto original=identity(cap);
    capnp::MallocMessageBuilder source,destination;
    auto src=source.getRoot<capnp::AnyPointer>();auto dst=destination.getRoot<capnp::AnyPointer>();
    fill(form,src,cap);cap=nullptr;
    auto paf=kj::newPromiseAndFulfiller<void>();
    auto policy=kj::refcounted<Boundary>(stats,kj::mv(paf.promise));
    std::optional<capnp::Orphan<capnp::AnyPointer>> orphan;
    while(events>>event) {
      auto calls=stats.calls;
      switch(event){
        case 1:case 2:orphan.emplace(copy(form,event==2,src.asReader(),destination.getOrphanage(),*policy));break;
        case 3:dst.adopt(kj::mv(*orphan));orphan.reset();break;
        case 4:case 5:orphan.emplace(copy(form,event==4,dst.asReader(),destination.getOrphanage(),*policy));break;
        case 6:orphan.reset();break;
        case 7:dst.clear();break;
        case 8:src.clear();break;
        case 9:paf.fulfiller->reject(KJ_EXCEPTION(FAILED,"revoked"));wait.poll();break;
        default:KJ_FAIL_REQUIRE("bad event",event);
      }
      KJ_REQUIRE(stats.calls==calls);
      unsigned s=src.isNull()?0:observe(read(form,src.asReader()),stats,original,wait);
      unsigned d=dst.isNull()?0:observe(read(form,dst.asReader()),stats,original,wait);
      unsigned o=orphan?observe(read(form,*orphan),stats,original,wait):0;
      wait.poll();std::cout<<s<<' '<<d<<' '<<o<<' '<<stats.live<<',';
    }
    orphan.reset();src.clear();dst.clear();wait.poll();KJ_REQUIRE(stats.live==0);
    std::cout<<'\n';
  }
}
