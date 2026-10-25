#include <chrono>
#include <string>

#include <kiyosi/kiyosi.hpp>

#include "pricing.hpp"

namespace kiyosi::benchmark_support {

void register_digital_cases()
{
    const Date start{std::chrono::year{2025} / 1 / 1}, expiry{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto add = [&](const std::string& denomination, const auto& option) {
        const auto register_engine = [&](const std::string& method, const auto& engine) {
            const auto prefix = "digital/" + denomination + "/" + method;
            register_price((prefix + "/price").c_str(), option, engine, context)->UseRealTime();
            register_calculation((prefix + "/delta_gamma").c_str(), [=] {
                return engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            },
                                 "price")
                ->UseRealTime();
        };
        for (const int steps : {256, 4096})
            register_engine("crr/" + std::to_string(steps), *make_cox_ross_rubinstein_digital_engine({steps}));
        for (const int paths : {65'536, 262'144}) {
            for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol})
                register_engine(std::string{"mc/cpu/"} + (sampling == MonteCarloSampling::sobol ? "sobol/" : "pseudo_random/") + std::to_string(paths),
                                *make_monte_carlo_digital_engine({.path_count = paths, .seed = 42, .sampling = sampling}));
#if KIYOSI_HAS_CUDA
            register_engine("mc/cuda/" + std::to_string(paths),
                            *make_monte_carlo_digital_engine({.path_count = paths, .seed = 42, .backend = MonteCarloBackend::cuda}));
#endif
        }
    };
    add("cash", *make_cash_or_nothing_option(OptionType::call, 100.0, 10.0, start, expiry));
    add("asset", *make_asset_or_nothing_option(OptionType::call, 100.0, start, expiry));
}

} // namespace kiyosi::benchmark_support
