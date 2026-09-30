#include "reflection-lookup.capnp.h"
#include <capnp/schema.h>
#include <capnp/schema-loader.h>
#include <capnp/message.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <sstream>
#include <unordered_map>
#include <variant>
using namespace reflectionLookup;
using Key=std::variant<capnp::StructSchema::Field,capnp::InterfaceSchema::Method,capnp::EnumSchema::Enumerant>;
unsigned canonical(unsigned i) { return i==2?1:i==7?6:i; }
unsigned keyHash(const Key& k) { return std::visit([](auto v){return v.hashCode();},k); }
// Deliberately force collisions: equality, not numerical hashes, identifies keys.
struct CollisionHash { size_t operator()(const Key&) const { return 0; } };
void populate(capnp::SchemaLoader& loader) {
  loader.loadCompiledTypeAndDependencies<Cache<capnp::Text>>();
  loader.loadCompiledTypeAndDependencies<Diamond<capnp::Text>>();
  loader.loadCompiledTypeAndDependencies<Tone>();
}
capnp::Schema bound(capnp::SchemaLoader& loader,uint64_t id,unsigned mode) {
  if(mode==0) return loader.get(id);
  if(mode==1) return loader.getUnbound(id);
  capnp::MallocMessageBuilder message;
  auto scope=message.initRoot<capnp::schema::Brand>().initScopes(1)[0];
  scope.setScopeId(id);
  if(mode==2) scope.initBind(0);
  else {
    auto value=scope.initBind(1)[0];
    if(mode==3) value.setUnbound();
    else if(mode==4) value.initType().setText();
    else if(mode==5) value.initType().setData();
    else if(mode==6) value.initType().initList().initElementType().setText();
    else KJ_FAIL_REQUIRE("bad brand mode");
  }
  return loader.get(id,message.getRoot<capnp::schema::Brand>());
}
struct Catalog {
  capnp::SchemaLoader a,b;
  Catalog() { populate(a); populate(b); }
  Key key(unsigned i,bool compiled) {
    if(i<=5) {
      auto schema=compiled && i!=5
        ? (i==4?capnp::Schema::from<Cache<capnp::Data>>():capnp::Schema::from<Cache<capnp::Text>>())
        : bound(i==5?b:a,capnp::typeId<Cache<capnp::Text>>(),i==4?5:4).asStruct();
      return i==1?schema.getFieldByName("first"):schema.getFields()[i==3?1:0];
    }
    if(i<=7) {
      auto schema=compiled
        ? (i==6?capnp::Schema::from<Root<capnp::Text>>():capnp::Schema::from<Diamond<capnp::Text>>())
        : bound(a,i==6?capnp::typeId<Root<capnp::Text>>():capnp::typeId<Diamond<capnp::Text>>(),4).asInterface();
      return schema.getMethodByName("match");
    }
    auto schema=compiled?capnp::Schema::from<Tone>():a.get(capnp::typeId<Tone>()).asEnum();
    return schema.getEnumerants()[i-8];
  }
};
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);
  std::ifstream inputs(argv[1]); KJ_REQUIRE(inputs.good());
  Catalog catalog;
  std::string line;
  while(std::getline(inputs,line)) {
    std::istringstream fields(line); char kind; unsigned a,b,c; fields>>kind>>a;
    if(kind=='P') {
      fields>>b>>c; auto left=catalog.key(b,a),right=catalog.key(c,a);
      bool equal=left==right;
      if(equal) KJ_REQUIRE(keyHash(left)==keyHash(right));
      std::cout<<equal<<'\n';
    } else if(kind=='B') {
      fields>>b;
      auto left=bound(catalog.a,capnp::typeId<Cache<capnp::Text>>(),a);
      auto right=bound(catalog.a,capnp::typeId<Cache<capnp::Text>>(),b);
      bool equal=left==right;
      if(equal) KJ_REQUIRE(left.hashCode()==right.hashCode());
      std::cout<<equal<<'\n';
    } else if(kind=='T') {
      std::unordered_map<Key,unsigned,CollisionHash> cache;
      unsigned selected=1,event;
      while(fields>>event) {
        unsigned result=0;
        if(event<=9) selected=event;
        else if(event==10) result=cache.insert_or_assign(catalog.key(selected,a),73).second;
        else if(event==11) result=cache.contains(catalog.key(selected,a));
        else if(event==12) result=cache.erase(catalog.key(selected,a));
        else KJ_FAIL_REQUIRE("bad event");
        unsigned mask=0;
        for(unsigned i=1;i<=9;++i) if(cache.contains(catalog.key(i,a))) mask|=1u<<(canonical(i)-1);
        std::cout<<result<<' '<<cache.size()<<' '<<mask<<',';
      }
      std::cout<<'\n';
    } else KJ_FAIL_REQUIRE("unknown input");
  }
}
