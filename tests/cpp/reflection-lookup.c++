#include "reflection-lookup.capnp.h"
#include <capnp/schema.h>
#include <capnp/schema-loader.h>
#include <capnp/message.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <sstream>
#include <vector>
#include <string>
using namespace reflectionLookup;
constexpr uint64_t ID_BASE=0xa100000000000000;
std::pair<std::vector<unsigned>,bool> shape(unsigned g,unsigned n) {
  std::vector<unsigned> parents;
  bool matched=false;
  if (g==1 || g==2) {
    if(n==1) parents={2,3}; else if(n==2) parents={4};
    matched=n==3 || n==4 || (g==2 && n==1);
  } else if(g==3) {
    if(n==1) parents={2,8}; else if(n<7) parents={n+1,n+1};
    matched=n==8;
  } else if(g==4) {
    if(n==1) parents={2,3}; else if(n==2 || n==3) parents={4};
    matched=n==4;
  } else if(n<g+1) parents={n+1,n+1};
  return {parents,matched};
}
void populate(capnp::SchemaLoader& loader,unsigned graph) {
  for(unsigned n=1;n<=8;++n) {
    capnp::MallocMessageBuilder message;
    auto node=message.initRoot<capnp::schema::Node>();
    node.setId(ID_BASE+n); node.setDisplayName("test:Interface"); node.setDisplayNamePrefixLength(5);
    auto iface=node.initInterface();
    auto [parents,matched]=shape(graph,n);
    auto supers=iface.initSuperclasses(parents.size());
    for(unsigned k=0;k<parents.size();++k) supers[k].setId(ID_BASE+parents[k]);
    if(matched) {
      auto method=iface.initMethods(1)[0]; method.setName("match");
      method.setParamStructType(ID_BASE+100); method.setResultStructType(ID_BASE+100);
    }
    loader.load(node);
  }
}
uint64_t query(capnp::InterfaceSchema schema,unsigned op,uint64_t needle,bool compiled) {
  if(op==0) {
    KJ_IF_SOME(method,schema.findMethodByName(needle==1?"match":"missing")) {
      KJ_REQUIRE(method==schema.getMethodByName(needle==1?"match":"missing"));
      return method.getContainingInterface().getProto().getId()-(compiled?0:ID_BASE);
    }
    return 0;
  }
  auto id=needle==0?UINT64_MAX:ID_BASE+needle;
  if(compiled) {
    if(needle==1) id=capnp::typeId<Root<capnp::Text>>();
    if(needle==2) id=capnp::typeId<Empty>();
    if(needle==3) id=schema.getProto().getId();
  }
  KJ_IF_SOME(found,schema.findSuperclass(id)) { KJ_REQUIRE(schema.extends(found)); return found.getProto().getId()-(compiled?0:ID_BASE); }
  return 0;
}
std::string hex(kj::StringPtr bytes) {
  static const char* digits="0123456789abcdef";
  std::string result;
  for(unsigned char b:bytes) { result+=digits[b>>4]; result+=digits[b&15]; }
  return result.empty()?"-":result;
}
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);
  std::ifstream inputs(argv[1]); KJ_REQUIRE(inputs.good());
  std::string line;
  while(std::getline(inputs,line)) {
    std::istringstream fields(line); char kind; unsigned a,b=0,c=0; fields>>kind>>a;
    try {
      if(kind=='Q') {
        fields>>b>>c; capnp::SchemaLoader loader; populate(loader,a);
        std::cout<<query(loader.get(ID_BASE+1).asInterface(),b,c,false)<<'\n';
      } else if(kind=='C') {
        fields>>b>>c;
        capnp::InterfaceSchema schemas[]={capnp::Schema::from<Diamond<capnp::Text>>(),
          capnp::Schema::from<Override<capnp::Text>>(),capnp::Schema::from<Limit63>(),
          capnp::Schema::from<Limit64>(),capnp::Schema::from<Limit65>()};
        std::cout<<query(schemas[a],b,c,true)<<'\n';
      } else if(kind=='E') {
        kj::StringPtr names[]={"zulu","alpha","middle","","missing","Alpha",kj::StringPtr("alpha\0",6),"☃"};
        unsigned ordinal=0;
        KJ_IF_SOME(e,capnp::Schema::from<Tone>().findEnumerantByName(names[a])) {ordinal=e.getOrdinal()+1;}
        std::cout<<ordinal<<'\n';
      } else if(kind=='N') {
        fields>>b; kj::StringPtr names[]={"","path:Outer.Inner","p:é.Name"};
        capnp::MallocMessageBuilder message; auto node=message.initRoot<capnp::schema::Node>();
        node.setId(900); node.setDisplayName(names[a]); node.setDisplayNamePrefixLength(b); node.initInterface();
        capnp::SchemaLoader loader; auto schema=loader.load(node);
        std::cout<<hex(schema.getShortDisplayName())<<' '<<hex(schema.getUnqualifiedName())<<'\n';
      } else KJ_FAIL_REQUIRE("unknown input");
    } catch(kj::Exception& e) {
      KJ_REQUIRE(e.getDescription().contains("inheritance graph"),e);
      std::cout<<9<<'\n';
    }
  }
}
