#include "european.hpp"
#include "quadrature.hpp"
#include "crr.hpp"
#include "digital.hpp"
#include "asian.hpp"
#include "fd.hpp"
#include "local_vol.hpp"
#include "barriers.hpp"
#include "monte_carlo.hpp"
#include "american_monte_carlo.hpp"
#include "heston.hpp"
#include <atomic>
#include <chrono>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <limits>

int main(int argc, char** argv) {
    try {
        if (argc == 2 && std::string(argv[1]) == "--info") {
            std::cout << "{\"library\":\"QuantLib\",\"version\":\"" << QL_VERSION
                      << "\",\"runtime_version\":\"unavailable: QuantLib exposes no runtime version API"
                      << "\",\"families\":[\"european_bsm\",\"digital_bsm\",\"geometric_asian_bsm\",\"arithmetic_asian_bsm\",\"fd_bsm\",\"fd_local_vol\",\"single_barrier_bsm\",\"binary_barrier_bsm\",\"touch_bsm\",\"double_barrier_bsm\",\"double_touch_bsm\",\"quadrature_vanilla\",\"quadrature_digital\",\"crr_vanilla\",\"crr_barrier\",\"mc_european_bsm\",\"mc_barrier_bsm\",\"mc_american_bsm\",\"heston_analytic\",\"heston_fd_european\",\"heston_fd_american\"]}\n";
            return 0;
        }
        bench::Request request(argc, argv);
        const auto family = request.take("family");
        const auto count = request.take("iterations");
        std::size_t end;
        if (count.empty() || count.front() == '-') throw std::runtime_error("iterations must be positive");
        const auto iterations = std::stoull(count, &end);
        if (end != count.size() || iterations == 0) throw std::runtime_error("iterations must be positive integer");
        bench::Price price;
        if (family == "mc_american_bsm") price = bench::american_monte_carlo(request);
        else if (family == "mc_european_bsm") price = bench::monte_carlo(request, false);
        else if (family == "mc_barrier_bsm") price = bench::monte_carlo(request, true);
        else if (family == "fd_bsm") price = bench::fd(request);
        else if (family == "fd_local_vol") price = bench::local_vol(request);
        else if (family == "european_bsm") price = bench::european(request);
        else if (family == "quadrature_vanilla") price = bench::quadrature(request, false);
        else if (family == "quadrature_digital") price = bench::quadrature(request, true);
        else if (family == "crr_vanilla") price = bench::crr(request, false);
        else if (family == "crr_barrier") price = bench::crr(request, true);
        else if (family == "digital_bsm") price = bench::digital(request);
        else if (family == "geometric_asian_bsm") price = bench::asian(request, false);
        else if (family == "arithmetic_asian_bsm") price = bench::asian(request, true);
        else if (family == "single_barrier_bsm" || family == "binary_barrier_bsm" || family == "touch_bsm" || family == "double_barrier_bsm" || family == "double_touch_bsm") price = bench::barriers(request, family);
        else if (family == "heston_analytic" || family == "heston_fd_european" || family == "heston_fd_american") price = bench::heston(request, family);
        else throw std::runtime_error("unknown family: " + family);
        request.finish();
        volatile double result = 0;
        for (int i = 0; i < 3; ++i) result = price();
        const auto start = std::chrono::steady_clock::now();
        for (std::uint64_t i = 0; i < iterations; ++i) {
            std::atomic_signal_fence(std::memory_order_seq_cst);
            result = price();
        }
        const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now() - start).count();
        if (!std::isfinite(result)) throw std::runtime_error("nonfinite native price");
        std::cout << std::setprecision(17) << "{\"price\":" << result << ",\"elapsed_ns\":" << elapsed
                  << ",\"iterations\":" << iterations << ",\"warmup_iterations\":3}\n";
    } catch (const std::exception& error) {
        std::cerr << "QuantLib benchmark: " << error.what() << '\n';
        return 1;
    }
}
