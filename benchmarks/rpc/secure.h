// Benchmark-only authenticated TLS. KJ has no native QUIC adapter at the pinned revision.
#pragma once
#include <kj/compat/tls.h>
#include <kj/debug.h>
#include <kj/timer.h>
#include <cstdlib>
#include <fstream>
#include <filesystem>
#include <sstream>
#include <string>

class BenchmarkTransport {
  inline static std::string defaultDirectory;
  static std::string read(const char* name) {
    const char* directory = std::getenv("CAPNTPROTO_BENCH_CERTS");
    if (directory == nullptr) directory = defaultDirectory.c_str();
    std::ifstream file(std::string(directory) + "/" + name);
    KJ_REQUIRE(file.good(), "cannot read benchmark certificate", name);
    return std::string(std::istreambuf_iterator<char>(file), {});
  }
  static kj::TlsContext::Options options(kj::TlsCertificate& root, kj::TlsKeypair& key, kj::Timer& timer) {
    const char* transport = std::getenv("CAPNTPROTO_BENCH_TRANSPORT");
    KJ_REQUIRE(transport == nullptr || std::string(transport) == "tls", "C++ fixture supports TLS only; QUIC is unsupported");
    kj::TlsContext::Options options;
    options.useSystemTrustStore = false;
    options.trustedCertificates = kj::arrayPtr(&root, 1);
    options.defaultKeypair = key;
    options.minVersion = kj::TlsVersion::TLS_1_3;
    options.timer = timer;
    options.acceptTimeout = 10 * kj::SECONDS;
    return options;
  }
  kj::TlsCertificate root;
  kj::TlsKeypair key;
  kj::TlsContext context;
public:
  static void setExecutable(const char* executable) {
    defaultDirectory = (std::filesystem::canonical(executable).parent_path() / "certificates").string();
  }
  explicit BenchmarkTransport(kj::Timer& timer)
    : root(read("ca.pem").c_str()), key{kj::TlsPrivateKey(read("server-key.pem").c_str()), kj::TlsCertificate(read("server.pem").c_str())},
      context(options(root, key, timer)) {}
  static std::string hostname() {
    const char* name = std::getenv("CAPNTPROTO_BENCH_SERVER_NAME");
    std::string value = name == nullptr ? "localhost" : name;
    KJ_REQUIRE(!value.empty() && value.find_first_not_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-:") == std::string::npos, "invalid benchmark server name");
    return value;
  }
  static std::string metadata() {
    return "{\"version\":1,\"transport\":\"tls-tcp\",\"tls\":\"1.3\",\"authentication\":\"server-certificate\",\"server_name\":\"" + hostname() + "\",\"early_data\":false,\"setup_timed\":false,\"alpn\":null}";
  }
  kj::Own<kj::NetworkAddress> address(kj::Own<kj::NetworkAddress> address) {
    return context.wrapAddress(kj::mv(address), hostname().c_str());
  }
};
