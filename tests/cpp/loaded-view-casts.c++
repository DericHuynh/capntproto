#include <capnp/dynamic.h>
#include <capnp/schema-loader.h>
#include <capnp/serialize.h>
#include <fcntl.h>
#include <iostream>
#include <kj/debug.h>
#include <string>
#include <unistd.h>
std::string hex(kj::ArrayPtr<const kj::byte> bytes) {
  std::string result;
  const char *digits = "0123456789abcdef";
  for (auto b : bytes) {
    result += digits[b >> 4];
    result += digits[b & 15];
  }
  return result;
}
std::string describe(capnp::DynamicValue::Reader value) {
  switch (value.getType()) {
  case capnp::DynamicValue::STRUCT: {
    auto s = value.as<capnp::DynamicStruct>();
    return describe(s.get("number")) + ":" + describe(s.get("payload"));
  }
  case capnp::DynamicValue::LIST: {
    std::string result;
    auto list = value.as<capnp::DynamicList>();
    for (unsigned i = 0; i < list.size(); ++i) {
      if (i)
        result += ',';
      result += describe(list[i]);
    }
    return result;
  }
  case capnp::DynamicValue::UINT:
    return std::to_string(value.as<uint32_t>());
  case capnp::DynamicValue::BOOL:
    return value.as<bool>() ? "1" : "0";
  case capnp::DynamicValue::ENUM:
    return std::to_string(value.as<capnp::DynamicEnum>().getRaw());
  case capnp::DynamicValue::TEXT:
    return hex(value.as<capnp::Text>().asBytes());
  case capnp::DynamicValue::DATA:
    return hex(value.as<capnp::Data>());
  default:
    KJ_FAIL_REQUIRE("unexpected value");
  }
}
void edit(capnp::DynamicStruct::Builder value, bool data) {
  value.set("number", uint32_t(99));
  if (data)
    value.set("payload", capnp::Data::Reader(
                             reinterpret_cast<const kj::byte *>("new"), 3));
  else
    value.set("payload", capnp::Text::Reader("new"));
}
void editRaw(capnp::AnyStruct::Builder raw, bool data) {
  auto bytes = raw.getDataSection();
  uint32_t wire = 99 ^ 17;
  for (unsigned i = 0; i < 4; ++i)
    bytes[i] = (wire >> (8 * i)) & 255;
  auto pointer = raw.getPointerSection()[0];
  if (data)
    pointer.setAs<capnp::Data>(
        capnp::Data::Reader(reinterpret_cast<const kj::byte *>("new"), 3));
  else
    pointer.setAs<capnp::Text>("new");
}
void editErased(capnp::DynamicList::Builder list, const std::string &target) {
  capnp::AnyList::Builder raw(kj::mv(list));
  if (target == "records")
    editRaw(raw.as<capnp::List<capnp::AnyStruct>>()[0], false);
  else if (target == "numbers")
    raw.as<capnp::List<uint32_t>>().set(0, 99);
  else if (target == "choices")
    raw.as<capnp::List<uint16_t>>().set(0, 65535);
  else if (target == "flags")
    raw.as<capnp::List<bool>>().set(0, false);
  else if (target == "texts")
    raw.as<capnp::List<capnp::Text>>().set(0, "new");
  else if (target == "blobs")
    raw.as<capnp::List<capnp::Data>>().set(
        0, capnp::Data::Reader(reinterpret_cast<const kj::byte *>("new"), 3));
  else if (target == "nested")
    raw.as<capnp::List<capnp::List<uint32_t>>>()[0].set(0, 99);
  else
    KJ_FAIL_REQUIRE(false);
}
int main(int argc, char **argv) {
  try {
    KJ_REQUIRE(argc == 6);
    int schemaFd = open(argv[1], O_RDONLY);
    KJ_REQUIRE(schemaFd >= 0);
    capnp::StreamFdMessageReader request(schemaFd);
    capnp::SchemaLoader loader;
    for (auto node :
         request.getRoot<capnp::schema::CodeGeneratorRequest>().getNodes())
      loader.load(node);
    auto type = loader.get(std::stoull(argv[2]))
                    .asStruct()
                    .getFieldByName(argv[4])
                    .getType();
    int seedFd = open(argv[3], O_RDONLY);
    KJ_REQUIRE(seedFd >= 0);
    capnp::StreamFdMessageReader seed(seedFd);
    capnp::MallocMessageBuilder message;
    message.setRoot(seed.getRoot<capnp::AnyPointer>());
    auto pointer = message.getRoot<capnp::AnyPointer>();
    std::string target = argv[4];
    bool erase = std::string(argv[5]) == "erase";
    bool write = erase || std::string(argv[5]) == "write";
    if (type.isStruct()) {
      if (write) {
        auto loaded =
            pointer.getAs<capnp::AnyStruct>().as<capnp::DynamicStruct>(
                type.asStruct());
        if (erase)
          editRaw(capnp::AnyStruct::Builder(kj::mv(loaded)), target == "data");
        else
          edit(loaded, target == "data");
      }
      std::cout << describe(
          pointer.asReader().getAs<capnp::AnyStruct>().as<capnp::DynamicStruct>(
              type.asStruct()));
    } else {
      auto schema = type.asList();
      if (write) {
        // Pinned C++ has no mutable AnyList::as<DynamicList>(schema). These
        // accepted layouts fit already, so the owning getter need not upgrade.
        auto list = pointer.getAs<capnp::DynamicList>(schema);
        if (erase)
          editErased(list, target);
        else if (target == "records")
          edit(list[0].as<capnp::DynamicStruct>(), false);
        else if (target == "numbers")
          list.set(0, uint32_t(99));
        else if (target == "flags")
          list.set(0, false);
        else if (target == "choices")
          list.set(0, capnp::DynamicEnum(schema.getEnumElementType(), 65535));
        else if (target == "texts")
          list.set(0, capnp::Text::Reader("new"));
        else if (target == "blobs")
          list.set(0, capnp::Data::Reader(
                          reinterpret_cast<const kj::byte *>("new"), 3));
        else if (target == "nested")
          list[0].as<capnp::DynamicList>().set(0, uint32_t(99));
        else
          KJ_FAIL_REQUIRE(false);
      }
      // Fresh read also avoids the pinned C++ projected-builder.asReader bug.
      std::cout << describe(
          pointer.asReader().getAs<capnp::AnyList>().as<capnp::DynamicList>(
              schema));
    }
    capnp::MallocMessageBuilder wrapper;
    wrapper.getRoot<capnp::AnyPointer>()
        .initAsAnyStruct(0, 1)
        .getPointerSection()[0]
        .set(pointer.asReader());
    // Canonicalize an envelope so either raw root kind is retained.
    auto canonical =
        wrapper.getRoot<capnp::AnyStruct>().asReader().canonicalize();
    std::cout << ' ' << hex(canonical.asBytes()) << '\n';
    close(seedFd);
    close(schemaFd);
    return 0;
  } catch (const kj::Exception &e) {
    std::cerr << e.getDescription().cStr() << '\n';
    return 2;
  }
}
