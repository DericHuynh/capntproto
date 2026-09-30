#include "enum-brand.capnp.h"
#include <capnp/schema.h>
#include <capnp/dynamic.h>
#include <capnp/message.h>
#include <capnp/schema-loader.h>
#include <kj/debug.h>
#include <iostream>
#include <fstream>
#include <sstream>
using namespace enumBrand;
using Tone = Scope<capnp::Text>::Tone;
struct Catalog {
  capnp::SchemaLoader registered, unregistered, foreign, extended;
  Catalog() {
    registered.loadCompiledTypeAndDependencies<Record>();
    registered.loadCompiledTypeAndDependencies<Scope<capnp::Text>::Inner<capnp::Data>>();
    for(auto schema:registered.getAllLoaded()) {
      unregistered.load(schema.getProto()); foreign.load(schema.getProto()); extended.load(schema.getProto());
    }
    capnp::MallocMessageBuilder message;
    auto original=capnp::Schema::from<Tone>().getProto();
    message.setRoot(original);
    auto members=message.getRoot<capnp::schema::Node>().getEnum().initEnumerants(3);
    for(unsigned i=0;i<2;++i) members.setWithCaveats(i,original.getEnum().getEnumerants()[i]);
    members[2].setName("future"); members[2].setCodeOrder(2);
    extended.load(message.getRoot<capnp::schema::Node>());
  }
  capnp::EnumSchema branded(capnp::SchemaLoader& loader, bool text) {
    capnp::MallocMessageBuilder message;
    auto scope=message.initRoot<capnp::schema::Brand>().initScopes(1)[0];
    scope.setScopeId(capnp::typeId<Scope<capnp::Text>>());
    auto ty=scope.initBind(1)[0].initType();
    if(text) ty.setText(); else ty.setData();
    return loader.get(capnp::typeId<Scope<capnp::Text>>(),message.getRoot<capnp::schema::Brand>())
      .asStruct().getFieldByName("tone").getType().asEnum();
  }
  capnp::DynamicValue::Reader value(unsigned selected, uint16_t ordinal) {
    if(selected==6) return ordinal;
    auto schema=capnp::Schema::from<Tone>();
    switch(selected) {
      case 0:break;
      case 1:schema=branded(unregistered,true);break;
      case 2:schema=branded(registered,false);break;
      case 3:schema=branded(foreign,true);break;
      case 4:schema=branded(extended,true);break;
      case 5:schema=registered.get(capnp::typeId<Scope<capnp::Text>::Inner<capnp::Data>::State>()).asEnum();break;
      default:KJ_FAIL_REQUIRE("bad selection");
    }
    return capnp::DynamicEnum(schema,ordinal);
  }
};
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2); std::ifstream inputs(argv[1]); KJ_REQUIRE(inputs.good());
  Catalog catalog; std::string line;
  while(std::getline(inputs,line)) {
    std::istringstream fields(line); unsigned event,selected=0,ordinal=0,result=0,inspected=0;
    capnp::MallocMessageBuilder message; auto target=message.initRoot<Record>(); target.setOther(77);
    while(fields>>event) {
      if(event>=1 && event<=7) selected=event-1;
      else if(event>=11 && event<=14) {unsigned ordinals[]={0,1,2,65535};ordinal=ordinals[event-11];}
      else if(event==21) {
        auto value=catalog.value(selected,ordinal);
        try {target.setTone(value.as<Tone>());result=1;}
        catch(kj::Exception& e) {
          KJ_REQUIRE(e.getDescription().contains(selected==6?"Value type mismatch":"Type mismatch in DynamicEnum.as()"),e); result=2;
        }
      } else if(event==22) {
        auto value=catalog.value(selected,ordinal);
        try {
          auto en=value.as<capnp::DynamicEnum>(); inspected=2;
          KJ_IF_SOME(member,en.getEnumerant()) {KJ_REQUIRE(member.getOrdinal()==ordinal);inspected=1;}
        } catch(kj::Exception& e) {
          KJ_REQUIRE(e.getDescription().contains("Value type mismatch"),e); KJ_REQUIRE(selected==6); inspected=3;
        }
      } else KJ_FAIL_REQUIRE("bad event",event);
      auto tag=target.isTone()?1u:0u;
      unsigned stored=tag?static_cast<uint16_t>(target.getTone()):target.getOther();
      std::cout<<selected<<' '<<ordinal<<' '<<tag<<' '<<stored<<' '<<result<<' '<<inspected<<',';
    }
    std::cout<<'\n';
  }
}
