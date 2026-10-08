#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::greek_value;

TEST_CASE("Analytic European prices preserve the large-volatility limit", "[audit-fixes]")
{
    const auto start = day(2025, 1, 6);
    const auto end = start + std::chrono::days{365};
    const kiyosi::AnalyticVanillaEngine engine;
    for (const double volatility : {1e155, 1e308}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.05, 0.02, volatility), 100.0, start);
        for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
            CAPTURE(volatility, type);
            const bool call = type == kiyosi::OptionType::call;
            const auto option = *kiyosi::make_european_option(type, 100.0, start, end);
            const double expected = 100.0 * std::exp(call ? -0.02 : -0.05);
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK_THAT(*price, Catch::Matchers::WithinAbs(expected, 1e-12));
            const auto result = engine.price_with_greeks(option, context, kiyosi::GreeksRequest{true});
            REQUIRE(result);
            CHECK_THAT(result->price(), Catch::Matchers::WithinAbs(expected, 1e-12));
            CHECK(result->all_finite());
            CHECK_THAT(greek_value(*result, kiyosi::Greek::delta),
                       Catch::Matchers::WithinAbs(call ? std::exp(-0.02) : 0.0, 1e-15));
            CHECK_THAT(greek_value(*result, kiyosi::Greek::rho),
                       Catch::Matchers::WithinAbs(call ? 0.0 : -std::exp(-0.05), 1e-15));
            CHECK(greek_value(*result, kiyosi::Greek::gamma) == 0.0);
            CHECK(greek_value(*result, kiyosi::Greek::vega) == 0.0);
        }
    }
}

TEST_CASE("Analytic charm retains dividend carry when density underflows")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const kiyosi::AnalyticVanillaEngine engine;
    for (const double dividend : {-0.02, 0.02}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.05, dividend, 0.001), 100.0, start);
        for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
            const double sign = type == kiyosi::OptionType::call ? 1.0 : -1.0;
            for (const bool in_the_money : {false, true}) {
                const double strike = (type == kiyosi::OptionType::call) == in_the_money ? 50.0 : 200.0;
                const auto option = *kiyosi::make_european_option(type, strike, start, end);
                const auto result = engine.price_with_greeks(option, context, {kiyosi::Greek::charm});
                REQUIRE(result);
                const double expected = in_the_money ? sign * dividend * std::exp(-dividend) / 365.0 : 0.0;
                CHECK_THAT(greek_value(*result, kiyosi::Greek::charm), Catch::Matchers::WithinAbs(expected, 1e-15));
                const auto numerical = kiyosi::calculate_numerical_greeks(engine, option, context);
                REQUIRE(numerical);
                CHECK_THAT(greek_value(*numerical, kiyosi::Greek::charm), Catch::Matchers::WithinAbs(expected, 2e-9));
            }
        }
    }
}

TEST_CASE("Bjerksund-Stensland respects European and immediate exercise bounds", "[audit-fixes]")
{
    const auto start = day(2025, 1, 6);
    const auto end = day(2026, 1, 6);
    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
        for (const auto inputs : {std::array{200.0, 100.0, 0.02, 0.02, 0.6},
                                  std::array{100.0, 100.0, 0.0, 0.02, 0.01},
                                  std::array{100.0, 100.0, 0.0, 0.1, 0.05},
                                  std::array{200.0, 100.0, 0.0, 0.1, 0.01}}) {
            const bool call = type == kiyosi::OptionType::call;
            const double spot = inputs[call ? 0 : 1];
            const double strike = inputs[call ? 1 : 0];
            const auto context = *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(inputs[call ? 2 : 3], inputs[call ? 3 : 2], inputs[4]), spot, start);
            const auto european = kiyosi::AnalyticVanillaEngine{}.price(
                *kiyosi::make_european_option(type, strike, start, end), context);
            const auto american = kiyosi::BjerksundStenslandVanillaEngine{}.price(
                *kiyosi::make_american_option(type, strike, start, end), context);
            CAPTURE(type, inputs);
            REQUIRE(european);
            REQUIRE(american);
            CHECK(*american >= *european - 1e-12);
            CHECK(*american >= std::max(call ? spot - strike : strike - spot, 0.0));
            CHECK_THAT(*american, Catch::Matchers::WithinAbs(std::max(*european, std::max(call ? spot - strike : strike - spot, 0.0)), 1e-10));
        }
    }
}

TEST_CASE("Analytic European calls and puts obey BSM identities")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto put = *kiyosi::make_european_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto call_result = *engine.price_with_greeks(call, context, kiyosi::GreeksRequest{true});
    const auto put_result = *engine.price_with_greeks(put, context, kiyosi::GreeksRequest{true});
    CHECK_THAT((call_result).price() - (put_result).price(),
               WithinAbs(100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04), 1e-12));
    CHECK_THAT(greek_value(call_result, kiyosi::Greek::delta) - greek_value(put_result, kiyosi::Greek::delta), WithinAbs(std::exp(-0.01), 1e-12));
    CHECK_THAT(greek_value(call_result, kiyosi::Greek::gamma), WithinAbs(greek_value(put_result, kiyosi::Greek::gamma), 1e-12));
    CHECK_THAT(greek_value(call_result, kiyosi::Greek::speed), WithinAbs(greek_value(put_result, kiyosi::Greek::speed), 1e-12));
    CHECK_THAT(greek_value(call_result, kiyosi::Greek::color), WithinAbs(greek_value(put_result, kiyosi::Greek::color), 1e-12));
    CHECK_THAT(greek_value(call_result, kiyosi::Greek::vega), WithinAbs(greek_value(put_result, kiyosi::Greek::vega), 1e-12));
    CHECK_THAT(greek_value(call_result, kiyosi::Greek::vanna), WithinAbs(greek_value(put_result, kiyosi::Greek::vanna), 1e-12));
    CHECK_THAT(greek_value(call_result, kiyosi::Greek::zomma), WithinAbs(greek_value(put_result, kiyosi::Greek::zomma), 1e-12));
}

TEST_CASE("Analytic European engine remains finite one day before expiry_date")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(option, context, kiyosi::GreeksRequest{true});
    REQUIRE(result.has_value());
    for (const auto& item : result->values_view()) {
        REQUIRE(item.has_value());
        const double value = item.value_or(std::numeric_limits<double>::quiet_NaN());
        CHECK(std::isfinite(value));
    }
}

TEST_CASE("Analytic European engine returns intrinsic value and unavailable Greeks at expiry_date")
{
    const auto expiry_date = day(2026, 1, 6);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, expiry_date, expiry_date);
    const auto put = *kiyosi::make_european_option(kiyosi::OptionType::put, 100.0, expiry_date, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto call_context = *kiyosi::make_pricing_context(parameters, 110.0, expiry_date);
    const auto put_context = *kiyosi::make_pricing_context(parameters, 90.0, expiry_date);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto call_result = *engine.price_with_greeks(call, call_context, kiyosi::GreeksRequest{true});
    const auto put_result = *engine.price_with_greeks(put, put_context, kiyosi::GreeksRequest{true});
    REQUIRE((call_result).price() == 10.0);
    REQUIRE((put_result).price() == 10.0);
    for (std::size_t index = 0; index < kiyosi::greek_count; ++index) {
        const auto measure = static_cast<kiyosi::Greek>(index);
        INFO("Greek index: " << index);
        CHECK_FALSE(call_result.has(measure));
        CHECK_FALSE(put_result.has(measure));
    }
}

TEST_CASE("Analytic European implied volatility recovers market volatility")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto price = *engine.price(option, context);
    const auto implied = kiyosi::implied_volatility(engine, option, context, price);
    REQUIRE(implied.has_value());
    CHECK_THAT(*implied, WithinAbs(0.3, 1e-7));
}

TEST_CASE("Analytic European implied volatility rejects invalid settings and prices")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto price = *engine.price(option, context);
    const auto invalid_bracket = kiyosi::implied_volatility(
        engine, option, context, price, kiyosi::ImpliedVolatilitySettings{1.0, 0.1});
    REQUIRE_FALSE(invalid_bracket.has_value());
    CHECK(invalid_bracket.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto non_finite_price = kiyosi::implied_volatility(
        engine, option, context, std::numeric_limits<double>::quiet_NaN());
    REQUIRE_FALSE(non_finite_price.has_value());
    CHECK(non_finite_price.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("Analytic European implied volatility reports solver failures")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto price = *engine.price(option, context);
    const auto unbracketed = kiyosi::implied_volatility(engine, option, context, 95.0);
    REQUIRE_FALSE(unbracketed.has_value());
    CHECK(unbracketed.error().category == kiyosi::ErrorCategory::unbracketed_volatility);

    const auto unconverged = kiyosi::implied_volatility(
        engine, option, context, price,
        kiyosi::ImpliedVolatilitySettings{.price_tolerance = 1e-15, .parameter_tolerance = 1e-15, .max_iterations = 1});
    REQUIRE_FALSE(unconverged.has_value());
    CHECK(unconverged.error().category == kiyosi::ErrorCategory::solver_non_convergence);
}

TEST_CASE("Analytic European engine reports invalid expiry_date")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto stale_context = *kiyosi::make_pricing_context(parameters, 100.0, expiry_date + std::chrono::days{1});
    const auto invalid_time_range = engine.price(option, stale_context);
    REQUIRE_FALSE(invalid_time_range.has_value());
    CHECK(invalid_time_range.error().category == kiyosi::ErrorCategory::invalid_time_range);
}

TEST_CASE("Analytic European expiry uses the validated timestamp boundary")
{
    const auto expiry = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, expiry, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto midnight = *kiyosi::make_pricing_context(parameters, 110.0, kiyosi::start_of_day(expiry));
    const auto noon = *kiyosi::make_pricing_context(parameters, 110.0, kiyosi::start_of_day(expiry) + std::chrono::hours{12});
    const kiyosi::AnalyticVanillaEngine engine;

    CHECK(*engine.price(option, midnight) == 10.0);
    const auto expired = engine.price(option, noon);
    REQUIRE_FALSE(expired);
    CHECK(expired.error().category == kiyosi::ErrorCategory::invalid_time_range);
}

TEST_CASE("Analytic European engine reports non-finite pricing and solver results")
{
    const auto valuation = day(2025, 1, 6);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto extreme_parameters = *kiyosi::make_bsm_parameters(-1000.0, 0.0, 0.3);
    const auto extreme_context = *kiyosi::make_pricing_context(extreme_parameters, 100.0, valuation);
    const auto long_option = *kiyosi::make_european_option(
        kiyosi::OptionType::put, 100.0, valuation, valuation + std::chrono::days{36500});
    const auto non_finite_result = engine.price(long_option, extreme_context);
    REQUIRE_FALSE(non_finite_result.has_value());
    CHECK(non_finite_result.error().category == kiyosi::ErrorCategory::invalid_result);

    const auto non_finite_solver = kiyosi::implied_volatility(
        engine, long_option, extreme_context, 1.0);
    REQUIRE_FALSE(non_finite_solver.has_value());
    CHECK(non_finite_solver.error().category == kiyosi::ErrorCategory::invalid_result);
}

TEST_CASE("Analytic European engine remains finite at near-zero volatility")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 1e-12);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(option, context, kiyosi::GreeksRequest{true});
    REQUIRE(result.has_value());
    CHECK(std::isfinite(result->price()));
    CHECK(std::isfinite(greek_value(*result, kiyosi::Greek::delta)));
    CHECK_FALSE(result->has(kiyosi::Greek::gamma));
}

TEST_CASE("Analytic European implied volatility recovers at-the-money volatility")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto call_price = *engine.price(call, call_context);
    const auto call_implied = kiyosi::implied_volatility(
        engine, call, call_context, call_price);
    REQUIRE(call_implied.has_value());
    CHECK_THAT(*call_implied, WithinAbs(0.35, 1e-7));
}

TEST_CASE("Analytic European implied volatility recovers deep-in-the-money volatility")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto deep_in_the_money = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 20.0, valuation, expiry_date);
    const auto deep_itm_price = *engine.price(deep_in_the_money, call_context);
    const auto deep_itm_implied = kiyosi::implied_volatility(
        engine, deep_in_the_money, call_context,
        deep_itm_price);
    REQUIRE(deep_itm_implied.has_value());
    CHECK_THAT(*deep_itm_implied, WithinAbs(0.35, 1e-5));
}

TEST_CASE("Analytic European implied volatility recovers deep-out-of-the-money volatility")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto deep_out_of_the_money = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 180.0, valuation, expiry_date);
    const auto deep_otm_price = *engine.price(deep_out_of_the_money, call_context);
    const auto deep_otm_implied = kiyosi::implied_volatility(
        engine, deep_out_of_the_money, call_context,
        deep_otm_price);
    REQUIRE(deep_otm_implied.has_value());
    CHECK_THAT(*deep_otm_implied, WithinAbs(0.35, 1e-6));
}

TEST_CASE("Analytic European implied volatility reports unbracketed quotes")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto invalid_quote = kiyosi::implied_volatility(engine, call, call_context, 0.01);
    REQUIRE_FALSE(invalid_quote.has_value());
    CHECK(invalid_quote.error().category == kiyosi::ErrorCategory::unbracketed_volatility);

    const auto negative_quote = kiyosi::implied_volatility(engine, call, call_context, -1.0);
    REQUIRE_FALSE(negative_quote.has_value());
    CHECK(negative_quote.error().category == kiyosi::ErrorCategory::unbracketed_volatility);

    const auto boundary_put = *kiyosi::make_european_option(
        kiyosi::OptionType::put, 120.0, valuation, expiry_date);
    const auto zero_carry = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.35);
    const auto zero_carry_context = *kiyosi::make_pricing_context(
        zero_carry, 100.0, valuation);
    const auto boundary = kiyosi::implied_volatility(
        engine, boundary_put, zero_carry_context, 20.0);
    REQUIRE(boundary.has_value());
    CHECK(*boundary == kiyosi::ImpliedVolatilitySettings{}.lower_bound);
}

TEST_CASE("Analytic European engine remains finite in deep tails")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 1'000'000.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(option, context, kiyosi::GreeksRequest{true});
    REQUIRE(result.has_value());
    for (const auto& item : result->values_view()) {
        REQUIRE(item.has_value());
        const double value = item.value_or(std::numeric_limits<double>::quiet_NaN());
        CHECK(std::isfinite(value));
    }
}

TEST_CASE("Analytic European engine remains finite for a short-dated low-volatility option")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::put, 200.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.05);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(option, context, kiyosi::GreeksRequest{true});
    REQUIRE(result.has_value());
    CHECK(std::isfinite(result->price()));
    CHECK(std::isfinite(greek_value(*result, kiyosi::Greek::delta)));
}

} // namespace
