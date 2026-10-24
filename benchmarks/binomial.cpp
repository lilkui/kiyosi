#include <chrono>
#include <string>

#include <kiyosi/kiyosi.hpp>

#include "pricing.hpp"

namespace kiyosi::benchmark_support {

void register_binomial_cases()
{
    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date expiry{std::chrono::year{2026} / 1 / 1};
    const auto context = make_pricing_context(make_bsm_parameters(0.04, 0.01, 0.2).value(), 100.0, start).value();
    const auto no_dividend = make_pricing_context(make_bsm_parameters(0.04, 0.0, 0.2).value(), 100.0, start).value();
    for (const int steps : {128, 256, 512, 1000, 4096}) {
        const auto engine = make_cox_ross_rubinstein_vanilla_engine({steps}).value();
        const auto add = [&](const char* exercise, const auto& option, const PricingContext& market) {
            const std::string prefix = std::string{"binomial/"} + exercise + "/" + std::to_string(steps);
            register_price((prefix + "/price").c_str(), option, engine, market)->UseRealTime();
            register_calculation((prefix + "/delta_gamma").c_str(), [=] {
                return engine.price_with_greeks(option, market, {Greek::delta, Greek::gamma});
            },
                                 "price")
                ->UseRealTime();
            register_calculation((prefix + "/mixed").c_str(), [=] {
                return engine.price_with_greeks(option, market, {Greek::vanna, Greek::zomma, Greek::charm, Greek::color});
            },
                                 "price")
                ->UseRealTime();
        };
        add("european", make_european_option(OptionType::call, 100.0, start, expiry).value(), context);
        add("american", make_american_option(OptionType::put, 100.0, start, expiry).value(), context);
        add("american_call/no_dividend", make_american_option(OptionType::call, 100.0, start, expiry).value(), no_dividend);
    }
}

} // namespace kiyosi::benchmark_support
