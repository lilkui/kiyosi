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
    const auto valuation = kiyosi::Date{std::chrono::year{2025} / 1 / 6};
    const auto expiry_date = kiyosi::Date{std::chrono::year{2026} / 1 / 6};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto put = *kiyosi::make_european_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto analytic_call = *kiyosi::AnalyticVanillaEngine{}.price(call, context);
    const auto analytic_put = *kiyosi::AnalyticVanillaEngine{}.price(put, context);
    const auto call_price = *analytic_call.require(kiyosi::RiskMeasure::price);
    const auto put_price = *analytic_put.require(kiyosi::RiskMeasure::price);
    CHECK(std::abs(call_price - put_price - (100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04))) < 1e-10);

    const auto in = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                  .strike = 100.0,
                                                  .effective_date = valuation,
                                                  .expiry_date = expiry_date,
                                                  .barrier_level = 130.0,
                                                  .barrier_type = kiyosi::BarrierType::up_and_in});
    const auto out = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                   .strike = 100.0,
                                                   .effective_date = valuation,
                                                   .expiry_date = expiry_date,
                                                   .barrier_level = 130.0,
                                                   .barrier_type = kiyosi::BarrierType::up_and_out});
    const auto barrier_in = *kiyosi::AnalyticBarrierEngine{}.price(in, context);
    const auto barrier_out = *kiyosi::AnalyticBarrierEngine{}.price(out, context);
    CHECK(std::abs(*barrier_in.require(kiyosi::RiskMeasure::price) +
                   *barrier_out.require(kiyosi::RiskMeasure::price) - call_price) < 2e-5);

    const auto coarse = *kiyosi::CoxRossRubinsteinVanillaEngine{32}.price(call, context);
    const auto fine = *kiyosi::CoxRossRubinsteinVanillaEngine{128}.price(call, context);
    CHECK(std::abs(*fine.require(kiyosi::RiskMeasure::price) - call_price) <
          std::abs(*coarse.require(kiyosi::RiskMeasure::price) - call_price));

    const kiyosi::MonteCarloVanillaEngine monte_carlo{20000, 2, 42};
    const auto first = *monte_carlo.price(call, context);
    const auto second = *monte_carlo.price(call, context);
    CHECK(*first.require(kiyosi::RiskMeasure::price) == *second.require(kiyosi::RiskMeasure::price));
}

TEST_CASE("Seeded Monte Carlo engines execute repeatably")
{
    const auto effective_date = kiyosi::Date{std::chrono::year{2025} / 1 / 1};
    const auto expiry_date = kiyosi::Date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective_date, expiry_date);
    const auto put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, effective_date, expiry_date);
    const std::vector<kiyosi::Date> observation_dates{effective_date + std::chrono::days{90},
                                                 effective_date + std::chrono::days{181},
                                                 effective_date + std::chrono::days{273}, expiry_date};
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.02,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 75.0,
                                                       .knock_out_levels = knock_outs,
                                                       .coupon_barrier_levels = {90.0, 90.0, 90.0, 90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = observation_dates,
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = coupons,
                                                         .maturity_coupon_rate = 0.08,
                                                         .initial_spot = 100.0,
                                                         .knock_in_level = 75.0,
                                                         .knock_out_levels = knock_outs,
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = observation_dates,
                                                         .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                         .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                         .principal_ratio = 1.0,
                                                         .effective_date = effective_date,
                                                         .expiry_date = expiry_date});
    const auto binary = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = coupons,
                                                               .maturity_coupon_rate = 0.08,
                                                               .initial_spot = 100.0,
                                                               .knock_out_levels = knock_outs,
                                                               .upper_strike = 100.0,
                                                               .lower_strike = 60.0,
                                                               .observation_dates = observation_dates,
                                                               .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                               .principal_ratio = 1.0,
                                                               .effective_date = effective_date,
                                                               .expiry_date = expiry_date});
    const auto ternary = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = coupons,
                                                                 .maturity_coupon_rate = 0.08,
                                                                 .minimum_coupon_rate = 0.02,
                                                                 .initial_spot = 100.0,
                                                                 .knock_in_level = 75.0,
                                                                 .knock_out_levels = knock_outs,
                                                                 .upper_strike = 100.0,
                                                                 .lower_strike = 60.0,
                                                                 .observation_dates = observation_dates,
                                                                 .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                                 .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                 .principal_ratio = 1.0,
                                                                 .effective_date = effective_date,
                                                                 .expiry_date = expiry_date});
    const auto require_repeatable = [&](const auto& instrument, const auto& engine) {
        const auto first = engine.price(instrument, context);
        const auto second = engine.price(instrument, context);
        REQUIRE(first.has_value());
        REQUIRE(second.has_value());
        REQUIRE(first->require(kiyosi::RiskMeasure::price).has_value());
        REQUIRE(second->require(kiyosi::RiskMeasure::price).has_value());
        CHECK(*first->require(kiyosi::RiskMeasure::price) == *second->require(kiyosi::RiskMeasure::price));
        CHECK(std::isfinite(*first->require(kiyosi::RiskMeasure::price)));
    };
    require_repeatable(call, kiyosi::MonteCarloVanillaEngine{20000, 252, 42});
    require_repeatable(put, kiyosi::MonteCarloVanillaEngine{20000, 50, 42});
    require_repeatable(phoenix, kiyosi::MonteCarloPhoenixEngine{{1000, 42}});
    require_repeatable(snowball, kiyosi::MonteCarloSnowballEngine{{1000, 42}});
    require_repeatable(binary, kiyosi::MonteCarloBinarySnowballEngine{{1000, 42}});
    require_repeatable(ternary, kiyosi::MonteCarloTernarySnowballEngine{{1000, 42}});
}
