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
    const auto volatility = implied_volatility(AnalyticVanillaEngine{}, *option, *context, *analytic);
    if (!volatility || std::abs(*volatility - parameters->volatility()) > 1e-8) return 1;
    const auto curve = make_rate_term_structure(start_of_day(start), {start_of_day(end)}, {std::exp(-0.04)});
    if (!curve) return 1;
    const auto curve_parameters = make_bsm_parameters(*curve, 0.01, 0.2);
    if (!curve_parameters) return 1;
    const auto curve_context = make_pricing_context(*curve_parameters, 100.0, start);
    if (!curve_context) return 1;
    const auto curve_price = AnalyticVanillaEngine{}.price(*option, *curve_context);
    if (!curve_price || std::abs(*curve_price - *analytic) > 1e-10) return 1;
    const auto touch = make_cash_one_touch_up(start, end, 120.0, 10.0);
    if (!touch) return 1;
    const auto touch_price = FiniteDifferenceBinaryBarrierEngine{}.price(*touch, *curve_context);
    if (!touch_price || *touch_price <= 0.0 || *touch_price > 10.0) return 1;
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
    if (std::bit_cast<std::uint64_t>(*serial) != std::bit_cast<std::uint64_t>(*parallel) || *serial <= 0.0) return 1;
    for (const bool antithetic : {false, true}) {
        auto sobol = MonteCarloSettings{4096, 50, 42, MonteCarloBackend::cpu, 1, MonteCarloSampling::sobol, antithetic};
        const auto sobol_serial = make_monte_carlo_vanilla_engine(sobol).value().price(*option, *context);
        sobol.thread_count = 4;
        const auto sobol_parallel = make_monte_carlo_vanilla_engine(sobol).value().price(*option, *context);
        if (!sobol_serial || !sobol_parallel || std::abs(*sobol_serial - *analytic) > 0.02) return 1;
        if (std::bit_cast<std::uint64_t>(*sobol_serial) != std::bit_cast<std::uint64_t>(*sobol_parallel)) return 1;
    }
    return 0;
} catch (const std::exception&) {
    return 1;
}
