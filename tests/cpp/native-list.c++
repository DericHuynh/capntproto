#include "native-list.capnp.h"
#include <capnp/schema-loader.h>
#include <capnp/dynamic.h>
#include <capnp/message.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <sstream>
using namespace nativeList;
using Numbers=capnp::List<uint32_t>;
using Choices=capnp::List<Choice>;
using Records=capnp::List<Item<capnp::Data>>;
using Nested=capnp::List<Records>;
const char* field(unsigned c) {return c==0?"numbers":c==1?"choices":c==2||c==4?"records":c==3?"nested":c==5||c==6?"caps":"deepNumbers";}
void populate(capnp::SchemaLoader& loader) {
  capnp::SchemaLoader native;native.loadCompiledTypeAndDependencies<Lists>();
  for(auto schema:native.getAllLoaded()) loader.load(schema.getProto());
}
void reg(capnp::SchemaLoader& loader,unsigned kind) {
  if(kind==0) loader.loadCompiledTypeAndDependencies<Choice>();
  else if(kind==1) loader.loadCompiledTypeAndDependencies<Item<capnp::Text>>();
  else if(kind==2) loader.loadCompiledTypeAndDependencies<Service>();
  else KJ_FAIL_REQUIRE("unknown registration");
}
void init(capnp::MallocMessageBuilder& message,capnp::SchemaLoader& loader,unsigned c) {
  auto root=message.initRoot<capnp::DynamicStruct>(loader.get(capnp::typeId<Lists>()).asStruct());
  auto list=root.init(field(c),1).as<capnp::DynamicList>();
  if(c==3 || c==7) list.init(0,1);
}
bool read(capnp::MallocMessageBuilder& message,capnp::SchemaLoader& loader,unsigned c) {
  auto list=message.getRoot<capnp::DynamicStruct>(loader.get(capnp::typeId<Lists>()).asStruct()).asReader().get(field(c)).as<capnp::DynamicList>();
  try {
    switch(c) {
      case 0:case 4:case 7: KJ_REQUIRE(list.as<Numbers>().size()==1);break;
      case 1:KJ_REQUIRE(list.as<Choices>().size()==1);break;
      case 2:KJ_REQUIRE(list.as<Records>().size()==1);break;
      case 3:KJ_REQUIRE(list.as<Nested>().size()==1);break;
      case 5:KJ_REQUIRE(list.as<capnp::List<Service>>().size()==1);break;
      case 6:KJ_REQUIRE(list.as<capnp::List<Other>>().size()==1);break;
      default:KJ_FAIL_REQUIRE("unknown case");
    }
    return true;
  }catch(kj::Exception& e) {KJ_REQUIRE(e.getDescription().contains("not compatible"),e);return false;}
}
bool write(capnp::MallocMessageBuilder& message,capnp::SchemaLoader& loader,unsigned c) {
  auto list=message.getRoot<capnp::DynamicStruct>(loader.get(capnp::typeId<Lists>()).asStruct()).get(field(c)).as<capnp::DynamicList>();
  try {
    switch(c) {
      case 0:case 4:case 7:list.as<Numbers>().set(0,1);break;
      case 1:list.as<Choices>().set(0,Choice::ONE);break;
      case 2:list.as<Records>()[0].setNumber(1);break;
      case 3:list.as<Nested>()[0][0].setNumber(1);break;
      default:KJ_FAIL_REQUIRE("non-writable case");
    }
    return true;
  }catch(kj::Exception& e) {KJ_REQUIRE(e.getDescription().contains("not compatible"),e);return false;}
}
unsigned value(capnp::MallocMessageBuilder& message,unsigned c) {
  auto root=message.getRoot<Lists>();
  switch(c) {
    case 0:return root.getNumbers()[0];case 1:return static_cast<uint16_t>(root.getChoices()[0]);
    case 2:case 4:return root.getRecords()[0].getNumber();
    case 3:return root.getNested()[0][0].getNumber();
    case 7:return root.getDeepNumbers()[0][0];default:return 0;
  }
}
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);std::ifstream inputs(argv[1]);KJ_REQUIRE(inputs.good());std::string line;
  while(std::getline(inputs,line)) {
    capnp::SchemaLoader loader;populate(loader);capnp::MallocMessageBuilder message;
    unsigned selected=0,event;init(message,loader,selected);std::istringstream events(line);
    while(events>>event) {
      unsigned result=0;
      if(event<=8) {selected=event-1;init(message,loader,selected);}
      else if(event<=11) reg(loader,event-9);
      else if(event==12) result=read(message,loader,selected);
      else if(event==13) result=write(message,loader,selected);
      else KJ_FAIL_REQUIRE("unknown event");
      std::cout<<result<<' '<<value(message,selected)<<',';
    }
    std::cout<<'\n';
  }
}
