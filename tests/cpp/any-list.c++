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
    context.getResults().setValue(17);return kj::READY_NOW;
  }
};
unsigned pointerValue(capnp::AnyPointer::Reader pointer,kj::WaitScope& wait) {
  switch(pointer.getPointerType()) {
    case capnp::PointerType::NULL_:return 0;
    case capnp::PointerType::LIST:{auto data=pointer.getAs<capnp::Data>();KJ_REQUIRE(data.size()==1 && data[0]==73);return 1;}
    case capnp::PointerType::CAPABILITY:KJ_REQUIRE(pointer.getAs<Service>().pingRequest().send().wait(wait).getValue()==17);return 2;
    default:KJ_FAIL_REQUIRE("unexpected pointer");
  }
}
void allocate(capnp::AnyPointer::Builder root,unsigned kind,unsigned count) {
  if(kind<7)root.initAsAnyList(static_cast<capnp::ElementSize>(kind),count);
  else {
    unsigned d[]={0,1,0,1,2},p[]={0,0,1,1,2};
    root.initAsListOfAnyStruct(d[kind-7],p[kind-7],count);
  }
}
std::array<unsigned,4> fields(capnp::AnyList::Reader list,kj::WaitScope& wait) {
  if(list.size()==0)return {0,0,0,0};
  if(list.getElementSize()==capnp::ElementSize::BIT) {
    auto bits=list.as<capnp::List<bool>>();return {bits[0],bits[bits.size()-1],0,0};
  }
  auto structs=list.as<capnp::List<capnp::AnyStruct>>();
  auto first=structs[0],last=structs[structs.size()-1];
  auto fd=first.getDataSection(),ld=last.getDataSection();
  auto fp=first.getPointerSection(),lp=last.getPointerSection();
  unsigned a=fd.size()?fd[0]:0,b=ld.size()?ld[ld.size()-1]:0;
  unsigned x=fp.size()?pointerValue(fp[0],wait):0,y=lp.size()?pointerValue(lp[lp.size()-1],wait):0;
  for(unsigned i=0;i<structs.size();++i) {
    auto data=structs[i].getDataSection();auto pointers=structs[i].getPointerSection();
    for(unsigned j=0;j<data.size();++j)if(!((i==0 && j==0) || (i==structs.size()-1 && j==data.size()-1)))KJ_REQUIRE(data[j]==0);
    for(unsigned j=0;j<pointers.size();++j)if(!((i==0 && j==0) || (i==structs.size()-1 && j==pointers.size()-1)))KJ_REQUIRE(pointers[j].isNull());
  }
  return {a,b,x,y};
}
unsigned snapshot(capnp::AnyPointer::Reader pointer,unsigned kind,kj::WaitScope& wait) {
  if(pointer.isNull())return 0;
  auto list=pointer.getAs<capnp::AnyList>();auto [a,b,x,y]=fields(list,wait);
  return 10000000+1000000*kind+100000*list.size()+10000*a+1000*b+10*x+y;
}
int main(int argc,char** argv) {
  if(argc==2 && std::string(argv[1])=="--projection-regression") {
    capnp::MallocMessageBuilder message;
    auto root=message.getRoot<capnp::AnyPointer>();root.initAsListOfAnyStruct(1,1,1);
    auto projected=root.getAs<capnp::List<capnp::Data>>();projected.init(0,1)[0]=73;
    auto fresh=root.asReader().getAs<capnp::List<capnp::Data>>()[0];
    KJ_REQUIRE(fresh.size()==1 && fresh[0]==73);
    // Pin the upstream discrepancy. Do not use this invalid shifted reader
    // as the expected semantics of the repaired Rust view.
    bool broken=false;
    try {auto bad=projected.asReader()[0];broken=bad.size()!=1 || bad[0]!=73;}
    catch(kj::Exception&){broken=true;}
    KJ_REQUIRE(broken,"pinned C++ projection behavior changed; review the oracle");
    std::cout<<1<<'\n';return 0;
  }
  KJ_REQUIRE(argc==2);std::ifstream input(argv[1]);KJ_REQUIRE(input.good());
  kj::EventLoop loop;kj::WaitScope wait(loop);std::string line;
  while(std::getline(input,line)) {
    std::istringstream events(line);unsigned event,kind=0,copiedKind=0;
    capnp::MallocMessageBuilder source,copy;
    auto src=source.getRoot<capnp::AnyPointer>(),dst=copy.getRoot<capnp::AnyPointer>();
    while(events>>event) {
      unsigned raw=0,rawBytes=0,projected=0,bitCast=0,bitRead=0;
      if(event>=1 && event<=36) {kind=(event-1)/3;allocate(src,kind,(event-1)%3);}
      else if(event==100 || event==101) {
        auto list=src.getAs<capnp::AnyList>();unsigned index=event==100?0:list.size()-1;
        if(kind==1)list.as<capnp::List<bool>>().set(index,true);
        else {auto data=list.as<capnp::List<capnp::AnyStruct>>()[index].getDataSection();data[event==100?0:data.size()-1]=1;}
      } else if(event>=200 && event<=205) {
        auto list=src.getAs<capnp::AnyList>().as<capnp::List<capnp::AnyStruct>>();
        auto pointers=list[event<203?0:list.size()-1].getPointerSection();
        auto pointer=pointers[event<203?0:pointers.size()-1];
        switch((event-200)%3) {
          case 0:pointer.clear();break;
          case 1:pointer.initAs<capnp::Data>(1)[0]=73;break;
          case 2:pointer.setAs<Service>(Service::Client(kj::heap<Server>()));break;
        }
      } else if(event==300) {dst.setAs<capnp::AnyList>(src.asReader().getAs<capnp::AnyList>());copiedKind=kind;}
      else if(event==301) {src.clear();kind=0;}
      else if(event==302) {
        try {rawBytes=src.asReader().getAs<capnp::AnyList>().getRawBytes().size();raw=1;}
        catch(kj::Exception&){raw=2;}
      } else if(event==303) {
        // C++ List<AnyPointer> has no root getter. List<Data> uses the same
        // pointer-element projection and exposes its ordinary typed getter.
        auto list=src.getAs<capnp::List<capnp::Data>>();
        list.init(0,1)[0]=73;
        // The pinned C++ builder.asReader() applies the data offset twice.
        // Re-read the root to observe the written value without that bug.
        auto fresh=src.asReader().getAs<capnp::List<capnp::Data>>();
        KJ_REQUIRE(fresh[0].size()==1 && fresh[0][0]==73);
        projected=1;
        auto erased=capnp::AnyList::Reader(fresh);
        KJ_REQUIRE(erased.totalSize().wordCount==src.asReader().getAs<capnp::AnyList>().totalSize().wordCount);
      } else if(event==304) {
        bool readable=false,writable=false;
        try {src.asReader().getAs<capnp::List<bool>>();readable=true;}catch(kj::Exception&){}
        try {src.getAs<capnp::List<bool>>();writable=true;}catch(kj::Exception&){}
        bitRead=readable?1:2;bitCast=writable?1:2;
      } else KJ_FAIL_REQUIRE("bad event",event);
      auto list=src.asReader().getAs<capnp::AnyList>();auto [a,b,x,y]=fields(list,wait);auto size=list.totalSize();
      std::cout<<static_cast<unsigned>(list.getElementSize())<<' '<<list.size()<<' '<<1000*a+100*b+10*x+y<<' '
               <<snapshot(dst.asReader(),copiedKind,wait)<<' '<<size.wordCount<<' '<<size.capCount<<' '<<raw<<' '<<rawBytes<<' '<<projected<<' '<<bitCast<<' '<<bitRead<<',';
    }
    std::cout<<'\n';
  }
}
