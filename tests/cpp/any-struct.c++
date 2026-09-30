#include "membrane-copy.capnp.h"
#include <capnp/any.h>
#include <capnp/message.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <array>
#include <fstream>
#include <iostream>
#include <sstream>
using namespace membraneCopy;
struct Server final:Service::Server {
  kj::Promise<void> ping(PingContext context) override {
    context.getResults().setValue(17); return kj::READY_NOW;
  }
};
unsigned pointerValue(capnp::AnyPointer::Reader pointer,kj::WaitScope& wait) {
  switch(pointer.getPointerType()) {
    case capnp::PointerType::NULL_:return 0;
    case capnp::PointerType::LIST: {
      auto data=pointer.getAs<capnp::Data>();KJ_REQUIRE(data.size()==1 && data[0]==73);return 1;
    }
    case capnp::PointerType::CAPABILITY: {
      KJ_REQUIRE(pointer.getAs<Service>().pingRequest().send().wait(wait).getValue()==17);return 2;
    }
    default:KJ_FAIL_REQUIRE("unexpected struct");
  }
}
std::array<unsigned,6> fields(capnp::AnyPointer::Reader pointer,kj::WaitScope& wait) {
  auto value=pointer.getAs<capnp::AnyStruct>();
  auto data=value.getDataSection();auto pointers=value.getPointerSection();
  unsigned d=data.size()/8,p=pointers.size();
  unsigned a=data.size()?data[0]:0,b=data.size()?data[data.size()-1]:0;
  unsigned x=p>0?pointerValue(pointers[0],wait):0,y=p>1?pointerValue(pointers[1],wait):0;
  for(unsigned i=1;i+1<data.size();++i)KJ_REQUIRE(data[i]==0);
  auto size=value.totalSize();
  KJ_REQUIRE(size.wordCount==d+p+(x==1)+(y==1));KJ_REQUIRE(size.capCount==(x==2)+(y==2));
  return {d,p,a,b,x,y};
}
unsigned snapshot(capnp::AnyPointer::Reader pointer,kj::WaitScope& wait) {
  if(pointer.isNull())return 0;
  auto [d,p,a,b,x,y]=fields(pointer,wait);
  return 100000+10000*d+1000*p+100*a+10*b+3*x+y;
}
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);std::ifstream input(argv[1]);KJ_REQUIRE(input.good());
  kj::EventLoop loop;kj::WaitScope wait(loop);std::string line;
  while(std::getline(input,line)) {
    std::istringstream events(line);unsigned event;
    capnp::MallocMessageBuilder source,copy;
    auto src=source.getRoot<capnp::AnyPointer>();auto dst=copy.getRoot<capnp::AnyPointer>();
    while(events>>event) {
      unsigned canon=0,cd=0,cp=0;
      if(event>=1 && event<=9)src.initAsAnyStruct((event-1)/3,(event-1)%3);
      else if(event==20 || event==21) {
        auto bytes=src.getAs<capnp::AnyStruct>().getDataSection();
        bytes[event==20?0:bytes.size()-1]=event==20?1:2;
      } else if(event>=30 && event<=35) {
        auto slot=src.getAs<capnp::AnyStruct>().getPointerSection()[(event-30)/3];
        switch((event-30)%3) {
          case 0:slot.clear();break;
          case 1:slot.initAs<capnp::Data>(1)[0]=73;break;
          case 2:slot.setAs<Service>(Service::Client(kj::heap<Server>()));break;
        }
      } else if(event==40) dst.setAs<capnp::AnyStruct>(src.asReader().getAs<capnp::AnyStruct>());
      else if(event==41)src.clear();
      else if(event==42) {
        try {
          auto words=src.asReader().getAs<capnp::AnyStruct>().canonicalize();
          kj::ArrayPtr<const capnp::word> segments[]={words.asPtr()};
          capnp::SegmentArrayMessageReader message(segments);
          auto value=message.getRoot<capnp::AnyStruct>();
          KJ_REQUIRE(message.isCanonical());
          KJ_REQUIRE(value.equals(src.asReader().getAs<capnp::AnyStruct>())==capnp::Equality::EQUAL);
          canon=1;cd=value.getDataSection().size()/8;cp=value.getPointerSection().size();
        } catch(kj::Exception&) {canon=2;}
      } else KJ_FAIL_REQUIRE("bad event",event);
      auto [d,p,a,b,x,y]=fields(src.asReader(),wait);
      std::cout<<d<<' '<<p<<' '<<1000*a+100*b+10*x+y<<' '<<snapshot(dst.asReader(),wait)
               <<' '<<canon<<' '<<cd<<' '<<cp<<',';
    }
    std::cout<<'\n';
  }
}
