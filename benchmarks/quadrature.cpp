#include <chrono>
#include <cmath>
#include <string>
#include <vector>

#include <benchmark/benchmark.h>
#include <kiyosi/kiyosi.hpp>

#include "pricing.hpp"
#include "pricing/detail/quadrature.hpp"

namespace kiyosi::benchmark_support {

void register_quadrature_cases()
{
    using namespace detail::quadrature;
    for (const int steps : {17, 512, 513, 1024}) {
        std::vector<double> nodes;
        std::vector<double> values;
        for (int index = 0; index <= steps; ++index) {
            const double node = std::lerp(-1.0, 1.0, static_cast<double>(index) / steps);
            nodes.push_back(node);
            values.push_back(std::exp(node));
        }
        const double deviation = 0.2 / std::sqrt(365.0);
        const std::string prefix = "quadrature/kernel/" + std::to_string(steps);
        for (const bool scalar : {false, true}) {
            benchmark::RegisterBenchmark(prefix + (scalar ? "/weights_scalar" : "/weights"), [nodes, deviation, scalar](benchmark::State& state) {
                for (auto _ : state) {
                    auto transition = scalar ? transition_weights_scalar(nodes, 0.0, deviation) : transition_weights(nodes, 0.0, deviation);
                    benchmark::DoNotOptimize(transition);
                }
            })->UseRealTime();
            benchmark::RegisterBenchmark(prefix + (scalar ? "/integrate_scalar" : "/integrate"), [nodes, values, deviation, scalar](benchmark::State& state) {
                const auto transition = transition_weights(nodes, 0.0, deviation);
                const auto curve = interpolate(nodes, values, 0.0, 0.0);
                std::vector<double> integrated(nodes.size());
                for (auto _ : state) {
                    if (scalar) integrate_curves_scalar(curve, transition, -0.03 / 365.0, integrated);
                    else integrate_curves(curve, transition, -0.03 / 365.0, integrated);
                    benchmark::DoNotOptimize(integrated);
                }
            })->UseRealTime();
        }
    }

    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date expiry{std::chrono::year{2025} / 4 / 1};
    for (const auto barrier_state : {AutocallableBarrierState::none, AutocallableBarrierState::knocked_in}) {
        const auto valuation = barrier_state == AutocallableBarrierState::none ? start : start + std::chrono::days{2};
        const auto context = make_pricing_context(make_bsm_parameters(0.03, 0.01, 0.2).value(), 100.0, valuation, all_days_calendar()).value();
        const auto note = make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = barrier_state, .effective_date = start, .expiry_date = expiry, .observations = {{Date{std::chrono::year{2025} / 2 / 1}, 110.0, 0.08}, {Date{std::chrono::year{2025} / 3 / 1}, 108.0, 0.08}, {expiry, 105.0, 0.08}}}).value();
        for (const int steps : {17, 512, 1024}) {
            const auto engine = make_quadrature_autocallable_engine({steps, 8.0}).value();
            const std::string prefix = std::string{"quadrature/snowball/"} + (barrier_state == AutocallableBarrierState::none ? "none/" : "knocked_in/") + std::to_string(steps);
            register_price((prefix + "/price").c_str(), note, engine, context)->UseRealTime();
            register_calculation((prefix + "/delta_gamma").c_str(), [=, note = note] { return engine.price_with_greeks(note, context, {Greek::delta, Greek::gamma}); }, "price")->UseRealTime();
            register_calculation((prefix + "/all").c_str(), [=, note = note] { return engine.price_with_greeks(note, context, true); }, "price")->UseRealTime();
        }
    }
}

} // namespace kiyosi::benchmark_support
