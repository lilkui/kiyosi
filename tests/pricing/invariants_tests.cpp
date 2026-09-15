#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <cmath>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

/// Engine-agnostic properties that must hold for any correct implementation: parity relations,
/// convergence direction, and seeded reproducibility.

TEST_CASE("Pricing reference public properties cover payoff, in-out, convergence, and seeded paths")
{
    const auto valuation = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 6};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto put = *kiyosi::make_european_option(kiyosi::option_type::put, 100.0, valuation, expiry);
    const auto analytic_call = *kiyosi::AnalyticVanillaEngine{}.price(call, context);
    const auto analytic_put = *kiyosi::AnalyticVanillaEngine{}.price(put, context);
    const auto call_price = *analytic_call.get(kiyosi::risk_measure::price);
    const auto put_price = *analytic_put.get(kiyosi::risk_measure::price);
    CHECK(std::abs(call_price - put_price - (100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04))) < 1e-10);

    const auto in = *kiyosi::make_barrier_option({.type = kiyosi::option_type::call,
                                                  .strike = 100.0,
                                                  .effective = valuation,
                                                  .expiry = expiry,
                                                  .barrier = 130.0,
                                                  .barrier_kind = kiyosi::barrier_type::up_and_in});
    const auto out = *kiyosi::make_barrier_option({.type = kiyosi::option_type::call,
                                                   .strike = 100.0,
                                                   .effective = valuation,
                                                   .expiry = expiry,
                                                   .barrier = 130.0,
                                                   .barrier_kind = kiyosi::barrier_type::up_and_out});
    const auto barrier_in = *kiyosi::AnalyticBarrierEngine{}.price(in, context);
    const auto barrier_out = *kiyosi::AnalyticBarrierEngine{}.price(out, context);
    CHECK(std::abs(*barrier_in.get(kiyosi::risk_measure::price) +
                   *barrier_out.get(kiyosi::risk_measure::price) - call_price) < 2e-5);

    const auto coarse = *kiyosi::CrrVanillaEngine{32}.price(call, context);
    const auto fine = *kiyosi::CrrVanillaEngine{128}.price(call, context);
    CHECK(std::abs(*fine.get(kiyosi::risk_measure::price) - call_price) <
          std::abs(*coarse.get(kiyosi::risk_measure::price) - call_price));

    const kiyosi::MonteCarloVanillaEngine monte_carlo{20000, 2, 42};
    const auto first = *monte_carlo.price(call, context);
    const auto second = *monte_carlo.price(call, context);
    CHECK(*first.get(kiyosi::risk_measure::price) == *second.get(kiyosi::risk_measure::price));
}

TEST_CASE("Seeded Monte Carlo engines execute repeatably")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 1};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, effective, expiry);
    const auto put = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, effective, expiry);
    const std::vector<kiyosi::date> observations{effective + std::chrono::days{90},
                                                 effective + std::chrono::days{181},
                                                 effective + std::chrono::days{273}, expiry};
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.02,
                                                       .initial_price = 100.0,
                                                       .knock_in_price = 75.0,
                                                       .knock_out_prices = knock_outs,
                                                       .coupon_barriers = {90.0, 90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observations = observations,
                                                       .frequency = kiyosi::observation_frequency::daily,
                                                       .touch_status = kiyosi::barrier_touch_status::none,
                                                       .principal_ratio = 1.0,
                                                       .effective = effective,
                                                       .expiry = expiry});
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = coupons,
                                                         .maturity_coupon_rate = 0.08,
                                                         .initial_price = 100.0,
                                                         .knock_in_price = 75.0,
                                                         .knock_out_prices = knock_outs,
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observations = observations,
                                                         .frequency = kiyosi::observation_frequency::daily,
                                                         .touch_status = kiyosi::barrier_touch_status::none,
                                                         .principal_ratio = 1.0,
                                                         .effective = effective,
                                                         .expiry = expiry});
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = coupons,
                                                               .maturity_coupon_rate = 0.08,
                                                               .initial_price = 100.0,
                                                               .knock_out_prices = knock_outs,
                                                               .upper_strike = 100.0,
                                                               .lower_strike = 60.0,
                                                               .observations = observations,
                                                               .touch_status = kiyosi::barrier_touch_status::none,
                                                               .principal_ratio = 1.0,
                                                               .effective = effective,
                                                               .expiry = expiry});
    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = coupons,
                                                                 .maturity_coupon_rate = 0.08,
                                                                 .minimal_coupon_rate = 0.02,
                                                                 .initial_price = 100.0,
                                                                 .knock_in_price = 75.0,
                                                                 .knock_out_prices = knock_outs,
                                                                 .upper_strike = 100.0,
                                                                 .lower_strike = 60.0,
                                                                 .observations = observations,
                                                                 .frequency = kiyosi::observation_frequency::daily,
                                                                 .touch_status = kiyosi::barrier_touch_status::none,
                                                                 .principal_ratio = 1.0,
                                                                 .effective = effective,
                                                                 .expiry = expiry});
    const auto require_repeatable = [&](const auto& instrument, const auto& engine) {
        const auto first = engine.price(instrument, context);
        const auto second = engine.price(instrument, context);
        REQUIRE(first.has_value());
        REQUIRE(second.has_value());
        REQUIRE(first->get(kiyosi::risk_measure::price).has_value());
        REQUIRE(second->get(kiyosi::risk_measure::price).has_value());
        CHECK(*first->get(kiyosi::risk_measure::price) == *second->get(kiyosi::risk_measure::price));
        CHECK(std::isfinite(*first->get(kiyosi::risk_measure::price)));
    };
    require_repeatable(call, kiyosi::MonteCarloVanillaEngine{20000, 252, 42});
    require_repeatable(put, kiyosi::MonteCarloVanillaEngine{20000, 50, 42});
    require_repeatable(phoenix, kiyosi::MonteCarloPhoenixEngine{{1000, 42}});
    require_repeatable(snowball, kiyosi::MonteCarloSnowballEngine{{1000, 42}});
    require_repeatable(binary, kiyosi::MonteCarloBinarySnowballEngine{{1000, 42}});
    require_repeatable(ternary, kiyosi::MonteCarloTernarySnowballEngine{{1000, 42}});
}
