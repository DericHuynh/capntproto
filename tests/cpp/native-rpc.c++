#include "native-rpc.capnp.h"
#include <capnp/schema-loader.h>
#include <capnp/dynamic.h>
#include <capnp/capability.h>
#include <capnp/message.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <optional>
#include <sstream>
using namespace nativeRpc;
using S=Service<capnp::Text>;
using D=Service<capnp::Data>;
using O=Outer<S>;
using Changed=Outer<D>;
struct Echo final:Derived<capnp::Text>::Server {
  bool& alive;unsigned& calls;
  Echo(bool& alive,unsigned& calls):alive(alive),calls(calls){alive=true;}
  ~Echo(){alive=false;}
  kj::Promise<void> ping(PingContext context) override {
    ++calls;context.getResults().setValue(context.getParams().getValue()+1);return kj::READY_NOW;
  }
};
struct Make final:Factory::Server {
  S::Client cap;
  explicit Make(S::Client cap):cap(kj::mv(cap)){}
  kj::Promise<void> open(OpenContext context) override {
    context.getResults().initInner().initBody().setCap(cap);return kj::READY_NOW;
  }
};
void populate(capnp::SchemaLoader& loader,bool registered) {
  capnp::SchemaLoader native;
  native.loadCompiledTypeAndDependencies<Factory>();
  native.loadCompiledTypeAndDependencies<Derived<capnp::Text>>();
  for(auto schema:native.getAllLoaded())loader.load(schema.getProto());
  if(registered){loader.loadCompiledTypeAndDependencies<Factory>();loader.loadCompiledTypeAndDependencies<Derived<capnp::Text>>();}
}
template <typename From,typename To> void checkList(bool allowed) {
  capnp::MallocMessageBuilder message;
  auto native=message.initRoot<capnp::AnyPointer>().initAs<capnp::List<From>>(0);
  auto list=capnp::toDynamic(native);
  bool reader=false,builder=false;
  try{list.asReader().template as<capnp::List<To>>();reader=true;}
  catch(kj::Exception& e){KJ_REQUIRE(e.getDescription().contains("not compatible"),e);}
  try{list.template as<capnp::List<To>>();builder=true;}
  catch(kj::Exception& e){KJ_REQUIRE(e.getDescription().contains("not compatible"),e);}
  KJ_REQUIRE(reader==allowed && builder==allowed);
}
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);std::ifstream input(argv[1]);KJ_REQUIRE(input.good());
  kj::EventLoop loop;kj::WaitScope wait(loop);std::string line;
  checkList<S,D>(true);checkList<S,Other>(false);checkList<Derived<capnp::Text>,S>(false);
  checkList<capnp::List<S>,capnp::List<Other>>(false);
  while(std::getline(input,line)) {
    capnp::SchemaLoader loader;
    bool alive=false;unsigned calls=0,returned=0,result=0,selected=0;
    std::optional<capnp::DynamicCapability::Client> source;
    std::optional<capnp::DynamicStruct::Pipeline> pipeline;
    std::optional<capnp::Capability::Client> native;
    std::optional<O::Pipeline> typed;
    std::optional<Changed::Pipeline> changed;
    std::istringstream events(line);unsigned event;
    while(events>>event) {
      if(event<=28) {
        unsigned setup=event-1;selected=setup%7+1;bool reg=(setup/7)%2,compiled=setup/14;
        populate(loader,reg);
        if(selected<=4) {
          capnp::Capability::Client client(kj::heap<Echo>(alive,calls));
          auto schema=selected==4?capnp::Schema::from<Derived<capnp::Text>>():capnp::Schema::from<S>();
          source.emplace(client.castAs<capnp::DynamicCapability>(compiled?schema:loader.get(schema.getProto().getId()).asInterface()));
        }else {
          Factory::Client factory(kj::heap<Make>(S::Client(kj::heap<Echo>(alive,calls))));
          auto schema=capnp::Schema::from<Factory>();
          auto client=factory.castAs<capnp::DynamicCapability>(compiled?schema:loader.get(schema.getProto().getId()).asInterface());
          auto call=client.newRequest("open").send();
          pipeline.emplace(kj::mv(call));
          call.wait(wait);
        }
      }else if(event==31 || event==32) {
        try {
          if(selected<=4) {
            if(event==31) {
              if(selected==2)native.emplace(source->as<D>());
              else if(selected==3)native.emplace(source->as<Other>());
              else native.emplace(source->as<S>());
            }else {
              if(selected==2)native.emplace(source->releaseAs<D>());
              else if(selected==3)native.emplace(source->releaseAs<Other>());
              else native.emplace(source->releaseAs<S>());
              source.reset();
            }
          }else {
            if(selected==5)typed.emplace(pipeline->releaseAs<O>());
            else if(selected==6)changed.emplace(pipeline->releaseAs<Changed>());
            else {pipeline->releaseAs<Wrong>();KJ_FAIL_REQUIRE("wrong pipeline accepted");}
            pipeline.reset();
          }
          result=1;
        }catch(kj::Exception& e){KJ_REQUIRE(e.getDescription().contains("not compatible"),e);result=2;}
      }else if(event==33){source.reset();pipeline.reset();}
      else if(event==34){native.reset();typed.reset();changed.reset();}
      else if(event==35) {
        auto client=native?native->castAs<S>():typed?typed->getInner().getBody().getCap():changed->getInner().getBody().getCap().castAs<S>();
        auto request=client.pingRequest();request.setValue(41);returned=request.send().wait(wait).getValue();
      }else KJ_FAIL_REQUIRE("unknown event");
      wait.poll();
      std::cout<<(source.has_value()||pipeline.has_value())<<' '<<(native.has_value()||typed.has_value()||changed.has_value())<<' '<<alive<<' '<<result<<' '<<calls<<' '<<returned<<',';
    }
    source.reset();pipeline.reset();native.reset();typed.reset();changed.reset();wait.poll();KJ_REQUIRE(!alive);
    std::cout<<'\n';
  }
}
