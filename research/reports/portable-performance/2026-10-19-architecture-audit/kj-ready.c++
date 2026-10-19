#include <kj/async.h>
#include <iostream>
#include <vector>
#include <cstdlib>

struct Errors final: kj::TaskSet::ErrorHandler {
  void taskFailed(kj::Exception&& error) override {
    std::cerr << error.getDescription().cStr() << "\n";
    std::abort();
  }
};

int main() {
  for (int repeat = 0; repeat < 5; ++repeat) {
    for (unsigned count: {1u, 2u, 8u, 16u}) {
      for (unsigned selected: {0u, count / 2, count - 1}) {
        kj::EventLoop loop;
        kj::WaitScope waitScope(loop);
        Errors errors;
        kj::TaskSet tasks(errors);
        std::vector<kj::Own<kj::PromiseFulfiller<void>>> fulfillers;
        std::vector<unsigned> callbacks(count, 0);
        for (unsigned i = 0; i < count; ++i) {
          auto pair = kj::newPromiseAndFulfiller<void>();
          fulfillers.push_back(kj::mv(pair.fulfiller));
          tasks.add(pair.promise.then([&, i]() { ++callbacks[i]; }));
        }
        loop.run();
        for (auto value: callbacks) if (value != 0) std::abort();
        fulfillers[selected]->fulfill();
        loop.run();
        unsigned siblings = 0;
        for (unsigned i = 0; i < count; ++i) {
          if (i != selected) siblings += callbacks[i];
        }
        if (callbacks[selected] != 1 || siblings != 0) std::abort();
        std::cout << "KJ_PROBE repeat=" << repeat << " parked=" << count
          << " selected=" << selected << " ready_callbacks=" << callbacks[selected]
          << " sibling_callbacks=" << siblings << "\n";
      }
    }
  }
}
