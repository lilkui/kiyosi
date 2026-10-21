#include <bit>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <exception>
#include <kiyosi/kiyosi.hpp>

int main()
try {
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date end{std::chrono::year{2026} / 1 / 1};
    const auto option = make_european_option(OptionType::call, 100.0, start, end);
    const auto parameters = make_bsm_parameters(0.04, 0.01, 0.2);
    if (!option || !parameters) return 1;
    const auto context = make_pricing_context(*parameters, 100.0, start);
    if (!context) return 1;
    const auto binomial = CoxRossRubinsteinVanillaEngine{}.price(*option, *context);
    const auto analytic = AnalyticVanillaEngine{}.price(*option, *context);
    if (!binomial || !analytic || std::abs(*binomial - *analytic) > 0.02) return 1;
    const auto accumulator = make_accumulator({100.0, 110.0, 1.0, 2.0, 3.0, start, end});
    if (!accumulator) return 1;
    const auto settled_context = make_pricing_context(*parameters, 110.0, end);
    if (!settled_context) return 1;
    const auto integral = QuadratureAccumulatorEngine{}.price(*accumulator, *settled_context);
    if (!integral || *integral != 30.0) return 1;
    const auto live_integral = make_quadrature_accumulator_engine({17, 8.0}).value().price(*accumulator, *context);
    if (!live_integral || !std::isfinite(*live_integral)) return 1;
    const auto serial = make_monte_carlo_vanilla_engine({4097, 50, 42, MonteCarloBackend::cpu, 1}).value().price(*option, *context);
    const auto parallel = make_monte_carlo_vanilla_engine({4097, 50, 42, MonteCarloBackend::cpu, 4}).value().price(*option, *context);
    if (!serial || !parallel) return 1;
    return std::bit_cast<std::uint64_t>(*serial) == std::bit_cast<std::uint64_t>(*parallel) && *serial > 0.0 ? 0 : 1;
} catch (const std::exception&) {
    return 1;
}
