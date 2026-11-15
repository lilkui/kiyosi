// Offline reference driver: one fresh public QuantLib calculation per grid.
#include "../local_vol.hpp"
#include <chrono>
#include <iomanip>
#include <iostream>
int main(int argc, char** argv) {
    try {
        if (std::string(QL_VERSION) != "1.44") throw std::runtime_error("QuantLib 1.44 required");
        bench::Request request(argc, argv);
        if (request.take("family") != "fd_local_vol" || request.take("iterations") != "1") throw std::runtime_error("reference driver expects local volatility, one call");
        auto price = bench::local_vol(request);
        request.finish();
        const auto start = std::chrono::steady_clock::now();
        const double value = price();
        const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now() - start).count();
        std::cout << std::setprecision(17) << "{\"price\":" << value << ",\"elapsed_ns\":" << elapsed << ",\"iterations\":1}" << '\n';
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
