#include <capnp/schema-parser.h>
#include <capnp/dynamic.h>
#include <capnp/message.h>
#include <capnp/compat/json.h>
#include <capnp/serialize-text.h>
#include <kj/filesystem.h>
#include <kj/debug.h>
#include <iostream>
#include <fstream>
#include <iterator>
#include "compat.capnp.h"
int main(int argc,char** argv) {
  try {
    KJ_REQUIRE(argc == 6 || argc == 7);
    auto fs=kj::newDiskFilesystem();
    auto imports=fs->getCurrent().openSubdir(kj::Path::parse("vendor/capnproto/c++/src"));
    const kj::ReadableDirectory* paths[]={imports.get()};
    capnp::SchemaParser parser;
    auto file=parser.parseFromDirectory(fs->getCurrent(),kj::Path::parse("crates/capntproto-compat/tests/compat.capnp"),paths);
    auto schema=file.getNested(argv[2]).asStruct();
    capnp::MallocMessageBuilder message;
    auto root=message.initRoot<capnp::DynamicStruct>(schema);
    capnp::JsonCodec json;
    if(std::string(argv[3]).find("annotations")!=std::string::npos) json.handleByAnnotation(schema);
    if(std::string(argv[3]).find("nondefault")!=std::string::npos) json.setHasMode(capnp::HasMode::NON_DEFAULT);
    capnp::TextCodec text;
    if(std::string(argv[3]).find("pretty")!=std::string::npos){json.setPrettyPrint(true);text.setPrettyPrint(true);}
    if(argc == 7) {
      std::ifstream seedFile(argv[6],std::ios::binary);
      std::string seed((std::istreambuf_iterator<char>(seedFile)),std::istreambuf_iterator<char>());
      text.decode(kj::StringPtr(seed.c_str(),seed.size()),root);
    }
    std::ifstream input(argv[4],std::ios::binary);
    std::string data((std::istreambuf_iterator<char>(input)),std::istreambuf_iterator<char>());
    auto mode = std::string(argv[1]);
    bool expectedError = mode == "text-error" || mode == "text-orphan-error";
    bool failed = false;
    try {
    if(std::string(argv[1])=="json") json.decode(kj::StringPtr(data.c_str(),data.size()),root);
    else if(mode=="text-orphan" || mode=="text-orphan-error") {
      // The public orphan overload takes a generated type, not a runtime Type.
      auto input = kj::StringPtr(data.c_str(),data.size());
      auto decode = [&]<typename T>() { message.adoptRoot(text.decode<T>(input, message.getOrphanage())); };
      auto name = std::string(argv[2]);
      if (name == "BoolBox") decode.template operator()<BoolBox>();
      else if (name == "NumberBox") decode.template operator()<NumberBox>();
      else if (name == "VoidBox") decode.template operator()<VoidBox>();
      else if (name == "UnionBox") decode.template operator()<UnionBox>();
      else if (name == "EmptyBox") decode.template operator()<EmptyBox>();
      else if (name == "EnumBox") decode.template operator()<EnumBox>();
      else if (name == "GroupBox") decode.template operator()<GroupBox>();
      else KJ_FAIL_REQUIRE("Unknown orphan test schema", name.c_str());
      root = message.getRoot<capnp::DynamicStruct>(schema);
    } else text.decode(kj::StringPtr(data.c_str(),data.size()),root);
    } catch(const kj::Exception& e) {
      if(!expectedError) throw;
      failed = true;
      std::cerr<<e.getDescription().cStr()<<'\n';
    }
    KJ_REQUIRE(failed == expectedError, "Expected decode to fail");
    auto canonical=capnp::AnyStruct::Reader(root.asReader()).canonicalize();
    std::ofstream out(argv[5]);
    const char* hex="0123456789abcdef";
    for(auto b:canonical.asBytes())out<<hex[b>>4]<<hex[b&15];
    out<<'\n'<<json.encode(root.asReader()).cStr()<<'\n'<<text.encode(root.asReader()).cStr()<<'\n';
    std::ofstream(std::string(argv[5])+".json")<<json.encode(root.asReader()).cStr();
    std::ofstream(std::string(argv[5])+".text")<<text.encode(root.asReader()).cStr();
    return 0;
  } catch(const kj::Exception& e) { std::cerr<<e.getDescription().cStr()<<'\n';return 2; }
}
