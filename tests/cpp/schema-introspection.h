// Included by schema-loader.c++; compares public reflection with the Rust API.
void inspectType(capnp::Type type) {
  using T = capnp::schema::Type;
  switch (type.which()) {
    case T::ANY_POINTER:
      KJ_IF_SOME(p, type.getBrandParameter()) {
        std::cout << "p" << p.scopeId << ":" << p.index;
      } else { std::cout << "any"; }
      break;
    case T::TEXT: std::cout << "text"; break;
    case T::DATA: std::cout << "data"; break;
    case T::LIST:
      std::cout << "list("; inspectType(type.asList().getElementType()); std::cout << ")"; break;
    case T::STRUCT: std::cout << "struct" << type.asStruct().getProto().getId(); break;
    case T::INTERFACE: std::cout << "interface" << type.asInterface().getProto().getId(); break;
    default: throw std::runtime_error("unexpected introspection fixture type");
  }
}
void inspectSchema(const std::string& label, capnp::Schema s, capnp::SchemaLoader& loader) {
  std::cout << "s " << label << " " << s.isBranded() << " "
      << (s.getGeneric() == loader.get(s.getProto().getId())) << " scopes";
  for (auto id: s.getGenericScopeIds()) std::cout << " " << id;
  std::cout << "\n";
  if (s.getProto().getIsGeneric()) {
    auto ids = s.getGenericScopeIds();
    // Inspect own and absent scopes even when the brand does not list them.
    kj::Vector<uint64_t> scopes;
    scopes.add(s.getProto().getId()); scopes.add(999);
    for (auto id: ids) if (id != s.getProto().getId()) scopes.add(id);
    for (auto id: scopes) {
      auto args = s.getBrandArgumentsAtScope(id);
      std::cout << "a " << label << " " << id << " " << args.size();
      for (auto index: {0u, 1u, 2u, 65535u}) { std::cout << " "; inspectType(args[index]); }
      std::cout << "\n";
    }
  }
  if (s.getProto().isStruct()) {
    auto structure = s.asStruct();
    std::cout << "u " << label << " union";
    for (auto f: structure.getUnionFields()) std::cout << " " << f.getIndex();
    std::cout << " other";
    for (auto f: structure.getNonUnionFields()) std::cout << " " << f.getIndex();
    std::cout << " tags";
    for (uint16_t tag: {0, 1, 2, 3, 42, 65535}) {
      KJ_IF_SOME(f, structure.getFieldByDiscriminant(tag)) { std::cout << " " << f.getIndex(); }
      else { std::cout << " -"; }
    }
    std::cout << "\n";
  }
}
void introspection(capnp::List<capnp::schema::Node>::Reader nodes, capnp::SchemaLoader& loader) {
  for (auto node: nodes) loader.load(node);
  for (auto node: nodes) {
    auto base = loader.get(node.getId());
    auto label = std::to_string(node.getId());
    inspectSchema(label + "/0", base, loader);
    inspectSchema(label + "/1", loader.getUnbound(node.getId()), loader);
    if (node.getParameters().size() > 0) {
      for (unsigned mode = 2; mode <= 6; ++mode) {
        capnp::MallocMessageBuilder message;
        auto scope = message.initRoot<capnp::schema::Brand>().initScopes(1)[0];
        scope.setScopeId(node.getId());
        switch (mode) {
          case 2: scope.initBind(0); break;
          case 3: scope.initBind(1)[0].setUnbound(); break;
          case 4: scope.initBind(1)[0].initType().setText(); break;
          default: scope.setInherit(); break;
        }
        inspectSchema(label + "/" + std::to_string(mode),
            loader.get(node.getId(), message.getRoot<capnp::schema::Brand>(),
              mode == 6 ? loader.getUnbound(node.getId()) : base), loader);
      }
    }
    if (node.isStruct()) {
      for (auto f: base.asStruct().getFields()) {
        auto t = f.getType();
        if (t.isStruct()) inspectSchema(label + "/field" + std::to_string(f.getIndex()), t.asStruct(), loader);
        if (t.isInterface()) inspectSchema(label + "/field" + std::to_string(f.getIndex()), t.asInterface(), loader);
      }
    }
  }
}
