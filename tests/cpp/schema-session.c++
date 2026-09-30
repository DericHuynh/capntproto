// Exercise Compiler's public lazy loader, the engine used by SchemaParser.
#include <capnp/compiler/compiler.h>
#include <capnp/compiler/module-loader.h>
#include <capnp/compiler/error-reporter.h>
#include <capnp/schema-parser.h>
#include <capnp/any.h>
#include <kj/debug.h>
#include <kj/filesystem.h>
#include <fstream>
#include <iostream>
#include <map>
#include <string>

class Errors final: public capnp::compiler::GlobalErrorReporter {
public:
  void addError(const kj::ReadableDirectory&, kj::PathPtr path,
      SourcePos start, SourcePos, kj::StringPtr message) override {
    KJ_FAIL_REQUIRE("schema error", path.toString(), start.byte, message);
  }
  bool hadErrors() override { return false; }
};

std::string hex(kj::ArrayPtr<const kj::byte> bytes) {
  const char* digits = "0123456789abcdef";
  std::string out;
  for (auto byte: bytes) { out += digits[byte >> 4]; out += digits[byte & 15]; }
  return out;
}

void dump(const capnp::compiler::Compiler& compiler, unsigned step, uint64_t result) {
  std::cout << "stage " << step << ' ' << result << '\n';
  std::map<uint64_t, capnp::Schema> schemas;
  for (auto schema: compiler.getLoader().getAllLoaded()) schemas.emplace(schema.getProto().getId(), schema);
  for (auto [id, schema]: schemas) {
    auto node = capnp::AnyStruct::Reader(schema.getProto()).canonicalize();
    auto info = capnp::AnyStruct::Reader(KJ_ASSERT_NONNULL(compiler.getSourceInfo(id))).canonicalize();
    std::cout << "node " << id << ' ' << hex(node.asBytes()) << ' ' << hex(info.asBytes()) << '\n';
  }
}

int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  auto fs = kj::newDiskFilesystem();
  // The public SchemaParser API resolves declaration aliases; establish this
  // separately from Compiler's more detailed lazy-loader exercise below.
  capnp::SchemaParser parser;
  auto parsed = parser.parseFromDirectory(fs->getCurrent(), kj::Path::parse("session-main.capnp"), nullptr);
  KJ_REQUIRE(KJ_ASSERT_NONNULL(parsed.findNested("Alias")) == parsed.getNested("Root"));
  Errors errors;
  capnp::compiler::ModuleLoader modules(errors);
  capnp::compiler::Compiler compiler;
  auto& module = KJ_ASSERT_NONNULL(modules.loadModule(fs->getCurrent(), kj::Path::parse("session-main.capnp")));
  auto main = compiler.add(module).getId();
  using C = capnp::compiler::Compiler;
  constexpr auto closure = C::NODE | C::PARENTS | C::DEPENDENCIES | C::DEPENDENCY_PARENTS | C::DEPENDENCY_DEPENDENCIES;
  compiler.eagerlyCompile(main, closure | C::CHILDREN);
  dump(compiler, 0, main);
  std::ifstream inputs(argv[1]);
  KJ_REQUIRE(inputs.good());
  uint64_t parent;
  std::string name;
  unsigned step = 0;
  while (inputs >> parent >> name) {
    uint64_t result = 0;
    if (name == "*") {
      auto scope = compiler.getLoader().get(parent);
      for (auto child: scope.getProto().getNestedNodes()) {
        compiler.getLoader().get(child.getId());
        compiler.eagerlyCompile(child.getId(), closure);
      }
      result = parent;
    } else {
      KJ_IF_SOME(id, compiler.lookup(parent, name.c_str())) {
        // This get() uses Compiler's real LazyLoadCallback. Complete the
        // dependency/parent closure and retain source-info for comparison.
        compiler.getLoader().get(id);
        compiler.eagerlyCompile(id, closure);
        result = id;
      }
    }
    dump(compiler, ++step, result);
  }
}
