#include <capnp/dynamic.h>
#include <capnp/schema-loader.h>
#include <capnp/serialize.h>
#include <kj/debug.h>
#include <fcntl.h>
#include <unistd.h>
#include <bit>
#include <cmath>
#include <fstream>
#include <iostream>
#include <string>

std::string decode(std::string text) {
  if (text == "_") return "";
  std::string out;
  for (size_t i=0; i<text.size(); i+=2) out += char(std::stoul(text.substr(i,2),nullptr,16));
  return out;
}
std::string hex(kj::ArrayPtr<const capnp::byte> bytes) {
  const char* digits="0123456789abcdef";
  std::string out;
  for (auto b: bytes) { out += digits[b>>4]; out += digits[b&15]; }
  return out;
}
std::string convert(capnp::DynamicValue::Reader value, unsigned target, capnp::StructSchema schema) {
  switch (target) {
    case 0: value.as<capnp::Void>(); return "void";
    case 1: return value.as<bool>() ? "bool1" : "bool0";
    case 2: return "i"+std::to_string(value.as<int8_t>());
    case 3: return "i"+std::to_string(value.as<int16_t>());
    case 4: return "i"+std::to_string(value.as<int32_t>());
    case 5: return "i"+std::to_string(value.as<int64_t>());
    case 6: return "u"+std::to_string(value.as<uint8_t>());
    case 7: return "u"+std::to_string(value.as<uint16_t>());
    case 8: return "u"+std::to_string(value.as<uint32_t>());
    case 9: return "u"+std::to_string(value.as<uint64_t>());
    case 10: {
      auto v=value.as<float>();
      return std::isnan(v) ? "nan32" : "f32:"+std::to_string(std::bit_cast<uint32_t>(v));
    }
    case 11: {
      auto v=value.as<double>();
      return std::isnan(v) ? "nan64" : "f64:"+std::to_string(std::bit_cast<uint64_t>(v));
    }
    case 12: {
      capnp::MallocMessageBuilder message;
      auto builder=message.initRoot<capnp::DynamicStruct>(schema);
      builder.set("kind",value);
      return "enum"+std::to_string(builder.get("kind").as<capnp::DynamicEnum>().getRaw());
    }
    case 13: return "text"+hex(value.as<capnp::Text>().asBytes());
    case 14: return "data"+hex(value.as<capnp::Data>());
    default: KJ_FAIL_REQUIRE("bad target");
  }
}
int main(int argc, char** argv) {
  KJ_REQUIRE(argc==5);
  int fd=open(argv[1],O_RDONLY); KJ_REQUIRE(fd>=0);
  capnp::StreamFdMessageReader request(fd);
  capnp::SchemaLoader loader;
  for (auto n: request.getRoot<capnp::schema::CodeGeneratorRequest>().getNodes()) loader.load(n);
  auto schema=loader.get(std::stoull(argv[3])).asStruct();
  auto foreign=loader.get(std::stoull(argv[4])).asEnum();
  auto kind=schema.getFieldByName("kind").getType().asEnum();
  std::ifstream inputs(argv[2]); KJ_REQUIRE(inputs.good());
  unsigned source,target; std::string token;
  while (inputs >> source >> token >> target) {
    std::string storage;
    capnp::DynamicValue::Reader value;
    switch (source) {
      case 0: value=int64_t(std::stoll(token)); break;
      case 1: value=uint64_t(std::stoull(token)); break;
      case 2: value=std::bit_cast<double>(uint64_t(std::stoull(token))); break;
      case 3: value=std::bit_cast<float>(uint32_t(std::stoul(token))); break;
      case 4: value=(token=="1"); break;
      case 5: value=capnp::VOID; break;
      case 6: storage=decode(token); value=capnp::Text::Reader(storage.data(),storage.size()); break;
      case 7: storage=decode(token); value=capnp::Data::Reader(reinterpret_cast<const capnp::byte*>(storage.data()),storage.size()); break;
      case 8: value=capnp::DynamicEnum(kind,std::stoul(token)); break;
      case 9: value=capnp::DynamicEnum(foreign,std::stoul(token)); break;
      default: KJ_FAIL_REQUIRE("bad source");
    }
    try { std::cout << convert(value,target,schema) << '\n'; }
    catch (const kj::Exception&) { std::cout << "error\n"; }
  }
  close(fd);
}
