#include "enum-brand.capnp.h"
#include <capnp/schema.h>
#include <capnp/dynamic.h>
#include <capnp/message.h>
#include <capnp/schema-loader.h>
#include <kj/debug.h>
#include <iostream>
#include <fstream>
#include <sstream>
#include <unordered_map>
using namespace enumBrand;
unsigned canonical(unsigned mode,unsigned i) {return i==4?4:mode==0?0:i==3?1:i;}
struct CollisionHash {size_t operator()(const capnp::EnumSchema&) const {return 0;}};
struct Catalog {
  capnp::SchemaLoader loader;
  Catalog() {
    loader.loadCompiledTypeAndDependencies<Scope<capnp::Text>>();
    loader.loadCompiledTypeAndDependencies<Scope<capnp::Text>::Inner<capnp::Data>>();
  }
  capnp::StructSchema target(unsigned mode,bool text=true) {
    if(mode==0) return text?capnp::Schema::from<Scope<capnp::Text>>():capnp::Schema::from<Scope<capnp::Data>>();
    auto id=capnp::typeId<Scope<capnp::Text>>();
    capnp::MallocMessageBuilder message;
    auto scope=message.initRoot<capnp::schema::Brand>().initScopes(1)[0]; scope.setScopeId(id);
    auto ty=scope.initBind(1)[0].initType();
    if(text) ty.setText();else ty.setData();
    return loader.get(id,message.getRoot<capnp::schema::Brand>()).asStruct();
  }
  capnp::EnumSchema key(unsigned mode,unsigned i) {
    if(i==0 || i==4) {
      auto schema=i==0?capnp::Schema::from<Scope<capnp::Text>::Tone>():capnp::Schema::from<Scope<capnp::Text>::Inner<capnp::Data>::State>();
      return mode==0?schema:loader.get(schema.getProto().getId()).asEnum();
    }
    auto ty=target(mode,i!=2).getFieldByName(i==3?"tones":"tone").getType();
    return i==3?ty.asList().getEnumElementType():ty.asEnum();
  }
};
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);std::ifstream inputs(argv[1]);KJ_REQUIRE(inputs.good());
  Catalog catalog;std::string line;
  while(std::getline(inputs,line)) {
    std::istringstream fields(line);char kind;unsigned mode;fields>>kind>>mode;
    if(kind=='P') {
      unsigned a,b;fields>>a>>b;auto left=catalog.key(mode,a),right=catalog.key(mode,b);
      if(left==right) KJ_REQUIRE(left.hashCode()==right.hashCode());
      std::cout<<(left==right)<<'\n';
    }else if(kind=='T') {
      capnp::MallocMessageBuilder message;
      auto target=message.initRoot<capnp::DynamicStruct>(catalog.target(mode));
      std::unordered_map<capnp::EnumSchema,unsigned,CollisionHash> cache;
      unsigned selected=0,event;
      while(fields>>event) {
        unsigned result=0;
        if(event<=5) selected=event-1;
        else if(event==6) result=cache.insert_or_assign(catalog.key(mode,selected),1).second;
        else if(event==7) result=cache.contains(catalog.key(mode,selected));
        else if(event==8 || event==9) {
          try {target.set("tone",capnp::DynamicEnum(catalog.key(mode,selected),event==8?1:65535));result=1;}
          catch(kj::Exception& e) {KJ_REQUIRE(e.getDescription().contains("mismatch"),e);}
        }else KJ_REQUIRE(event==10+mode);
        unsigned mask=0;
        for(unsigned i=0;i<5;++i) if(cache.contains(catalog.key(mode,i))) mask|=1u<<canonical(mode,i);
        std::cout<<result<<' '<<cache.size()<<' '<<mask<<' '<<target.get("tone").as<capnp::DynamicEnum>().getRaw()<<',';
      }
      std::cout<<'\n';
    }else KJ_FAIL_REQUIRE("unknown input");
  }
}
