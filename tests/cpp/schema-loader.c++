#include <capnp/schema-loader.h>
#include <capnp/schema.capnp.h>
#include <capnp/serialize.h>
#include <capnp/dynamic.h>
#include <kj/exception.h>
#include <iostream>
#include <string>
#include <stdexcept>
#include <kj/vector.h>
#include "schema-introspection.h"

void describe(capnp::DynamicValue::Reader value) {
  using V = capnp::DynamicValue;
  switch (value.getType()) {
    case V::VOID: std::cout << "void"; break;
    case V::BOOL: std::cout << "b" << value.as<bool>(); break;
    case V::INT: std::cout << "i" << value.as<int64_t>(); break;
    case V::UINT: std::cout << "u" << value.as<uint64_t>(); break;
    case V::FLOAT: std::cout << "f" << value.as<double>(); break;
    case V::TEXT: std::cout << "t:" << value.as<capnp::Text>().cStr(); break;
    case V::DATA: {
      auto data = value.as<capnp::Data>();
      std::cout << "d:[";
      for (unsigned i = 0; i < data.size(); ++i) {
        if (i) std::cout << ", ";
        std::cout << unsigned(data[i]);
      }
      std::cout << "]";
      break;
    }
    case V::ENUM: {
      auto e = value.as<capnp::DynamicEnum>();
      std::cout << "e" << e.getSchema().getProto().getId() << ":" << e.getRaw();
      break;
    }
    case V::STRUCT: {
      auto s = value.as<capnp::DynamicStruct>();
      std::cout << "s" << s.getSchema().getProto().getId() << ":[";
      bool first = true;
      for (auto field: s.getSchema().getFields()) {
        if (!first) std::cout << ";";
        first = false;
        describe(s.get(field));
      }
      std::cout << "]";
      break;
    }
    case V::LIST: {
      auto list = value.as<capnp::DynamicList>();
      std::cout << "[";
      for (unsigned i = 0; i < list.size(); ++i) {
        if (i) std::cout << ";";
        describe(list[i]);
      }
      std::cout << "]";
      break;
    }
    default: throw std::runtime_error("unsupported differential fixture value");
  }
}

void annotations(capnp::SchemaLoader& loader, capnp::Schema scope,
    const std::string& member, capnp::List<capnp::schema::Annotation>::Reader list) {
  for (auto annotation: list) {
    auto declaration = loader.get(annotation.getId(), annotation.getBrand(), scope);
    auto type = loader.getType(declaration.getProto().getAnnotation().getType(), declaration);
    auto value = annotation.getValue();
    std::cout << "a " << scope.getProto().getId() << " " << member << " " << annotation.getId() << " ";
    switch (type.which()) {
      case capnp::schema::Type::TEXT: describe(value.getText()); break;
      case capnp::schema::Type::UINT32: describe(value.getUint32()); break;
      case capnp::schema::Type::STRUCT:
        describe(value.getStruct().as<capnp::DynamicStruct>(type.asStruct())); break;
      default: throw std::runtime_error("unsupported differential fixture annotation");
    }
    std::cout << "\n";
  }
}

int main(int argc, char** argv) {
  capnp::StreamFdMessageReader input(0);
  auto nodes = input.getRoot<capnp::schema::CodeGeneratorRequest>().getNodes();
  capnp::SchemaLoader loader;
  if (argc == 2 && std::string(argv[1]) == "introspection") {
    introspection(nodes, loader);
    return 0;
  }
  if (argc == 2 && std::string(argv[1]) == "metadata") {
    for (auto node: nodes) loader.load(node);
    for (auto node: nodes) {
      auto schema = loader.get(node.getId());
      if (node.isConst()) {
        std::cout << "c " << node.getId() << " ";
        describe(schema.asConst());
        std::cout << "\n";
      }
      annotations(loader, schema, "node", node.getAnnotations());
      if (node.isStruct()) {
        for (auto field: schema.asStruct().getFields())
          annotations(loader, schema, "field" + std::to_string(field.getIndex()), field.getProto().getAnnotations());
      }
      if (node.isEnum()) {
        for (auto e: schema.asEnum().getEnumerants())
          annotations(loader, schema, "enumerant" + std::to_string(e.getOrdinal()), e.getProto().getAnnotations());
      }
      if (node.isInterface()) {
        for (auto m: schema.asInterface().getMethods())
          annotations(loader, schema, "method" + std::to_string(m.getOrdinal()), m.getProto().getAnnotations());
      }
    }
    return 0;
  }
  for (auto node: nodes) {
    try {
      auto loaded = loader.load(node).getProto();
      unsigned size = 0;
      if (loaded.isStruct()) size = loaded.getStruct().getFields().size();
      if (loaded.isEnum()) size = loaded.getEnum().getEnumerants().size();
      if (loaded.isInterface()) size = loaded.getInterface().getMethods().size();
      std::cout << "ok " << loaded.getId() << " " << static_cast<unsigned>(loaded.which()) << " " << size << "\n";
    } catch (const kj::Exception&) { std::cout << "error\n"; }
  }
}
