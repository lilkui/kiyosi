#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <limits>

#include "support/reference_harness.hpp"
#include "pricing/detail/math.hpp"

using kiyosi::test::measures;

TEST_CASE("Digital prices preserve subnormal volatility", "[audit-fixes]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date end{std::chrono::year{2026} / 1 / 1};
    for (const double sigma : {std::numeric_limits<double>::denorm_min(), 1e-320, 1e-310}) {
        for (const auto remaining : {std::chrono::microseconds{std::chrono::days{1}}, std::chrono::microseconds{1}}) {
            for (const auto type : {OptionType::call, OptionType::put}) {
                const auto cash = *make_cash_or_nothing_option(type, 100.0, 10.0, start, end);
                const auto asset = *make_asset_or_nothing_option(type, 100.0, start, end);
                for (const double spot : {99.0, 100.0, 101.0}) {
                    CAPTURE(sigma, remaining.count(), type, spot);
                    const double probability = spot == 100.0 ? 0.5 : static_cast<double>((spot > 100.0) == (type == OptionType::call));
                    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), spot, start_of_day(end) - remaining);
                    const auto check = [&](const auto& engine, const auto& option, double expected) {
                        const auto price = engine.price(option, context);
                        REQUIRE(price);
                        CHECK(*price == Catch::Approx(expected).margin(1e-10));
                    };
                    check(AnalyticDigitalEngine{}, cash, 10.0 * probability);
                    check(QuadratureDigitalEngine{}, cash, 10.0 * probability);
                    check(AnalyticDigitalEngine{}, asset, spot * probability);
                    check(QuadratureDigitalEngine{}, asset, spot * probability);
                }
            }
        }
    }
}

TEST_CASE("Digital gamma survives underflowed volatility time", "[audit-fixes]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date end{std::chrono::year{2026} / 1 / 1};
    const auto valuation = start_of_day(end) - std::chrono::microseconds{1};
    const double root_time = std::sqrt(1e-6 / (365.0 * 86400.0));
    constexpr double spot = 1e300;
    const AnalyticDigitalEngine engine;
    for (const double sigma : {std::numeric_limits<double>::denorm_min(), 1e-320, 1e-310}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), spot, valuation);
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(sigma, type);
            const double sign = type == OptionType::call ? 1.0 : -1.0;
            const double asset_gamma = sign * 0.5 * detail::inverse_sqrt_two_pi / spot / sigma / root_time;
            const auto cash = *make_cash_or_nothing_option(type, spot, 10.0, start, end);
            const auto asset = *make_asset_or_nothing_option(type, spot, start, end);
            const auto check = [&](const auto& option, double expected_gamma, double expected_price) {
                const auto result = engine.price_with_greeks(option, context, {Greek::gamma});
                REQUIRE(result);
                const auto gamma = result->require(Greek::gamma);
                REQUIRE(gamma);
                CHECK(*gamma == Catch::Approx(expected_gamma).epsilon(1e-11).margin(0.0));
                CHECK(result->price() == expected_price);
            };
            check(asset, asset_gamma, 0.5 * spot);
            check(cash, -asset_gamma * (10.0 / spot), 5.0);
            const auto overflow = engine.price_with_greeks(asset, context, {Greek::delta});
            REQUIRE_FALSE(overflow);
            CHECK(overflow.error().category == ErrorCategory::invalid_result);
        }
    }
}

TEST_CASE("Analytic digital prices preserve extreme spot-strike ratios", "[audit-fixes]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const auto expiry = start + std::chrono::days{365};
    const auto parameters = *make_bsm_parameters(0.0, 0.0, 50.0);
    const AnalyticDigitalEngine engine;
    // Independent normal-tail reference for d = 25 - log(1e400) / 50.
    constexpr double probability = 0.9999999999763696;
    const auto check = [&](const auto& option, double spot, double scale) {
        const auto context = *make_pricing_context(parameters, spot, start);
        const auto price = engine.price(option, context);
        REQUIRE(price);
        CHECK(*price / scale == Catch::Approx(probability).epsilon(1e-12));
        const auto joint = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(joint);
        CHECK(joint->price() == *price);
        CHECK(joint->all_finite());
    };
    check(*make_cash_or_nothing_option(OptionType::put, 1e-200, 1.0, start, expiry), 1e200, 1.0);
    check(*make_asset_or_nothing_option(OptionType::call, 1e200, start, expiry), 1e-200, 1e-200);
}

TEST_CASE("Digital finite differences reject prices outside payoff bounds", "[audit-fixes]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 6};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto expiry = start + std::chrono::days{365};
    const FiniteDifferenceDigitalEngine engine{10'000, 1};
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto cash = *make_cash_or_nothing_option(type, 100.5, 100.0, start, expiry);
        const auto asset = *make_asset_or_nothing_option(type, 100.5, start, expiry);
        const auto check = [&](const auto& option) {
            const auto price = engine.price(option, context);
            REQUIRE_FALSE(price);
            CHECK(price.error().category == ErrorCategory::invalid_result);
            const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
            REQUIRE_FALSE(joint);
            CHECK(joint.error().category == ErrorCategory::invalid_result);
            const auto stable = FiniteDifferenceDigitalEngine{10'000, 1, FiniteDifferenceScheme::implicit_euler}.price(option, context);
            REQUIRE(stable);
            CHECK(*stable > 40.0);
            CHECK(*stable < 60.0);
        };
        check(cash);
        check(asset);
    }
    const auto volatile_context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 3.0), 100.0, start);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto cash = *make_cash_or_nothing_option(type, 110.0, 100.0, start, start + std::chrono::days{1825});
        const auto price = FiniteDifferenceDigitalEngine{}.price(cash, volatile_context);
        REQUIRE_FALSE(price);
        CHECK(price.error().category == ErrorCategory::invalid_result);
    }
}

TEST_CASE("Digital finite differences allow payoff-bound roundoff", "[audit-fixes]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 6};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto option = *make_cash_or_nothing_option(OptionType::call, 20.0, 100.0, start, start + std::chrono::days{1});
    const auto price = FiniteDifferenceDigitalEngine{}.price(option, context);
    REQUIRE(price);
    CHECK(*price >= 0.0);
    CHECK(*price <= 100.0 * std::exp(-0.05 / 365.0));
    CHECK(*price == Catch::Approx(99.98630230808251));
}

TEST_CASE("Digital analytic Greeks preserve tiny-volatility limits and scaled tails", "[audit-fixes]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const Date end{std::chrono::year{2026} / 1 / 1};
    const AnalyticDigitalEngine engine;
    constexpr double sigma = 1e-200;
    for (const auto type : {OptionType::call, OptionType::put}) {
        const double sign = type == OptionType::call ? 1.0 : -1.0;
        const auto cash = *make_cash_or_nothing_option(type, 100, 1, start, end);
        const auto asset = *make_asset_or_nothing_option(type, 100, start, end);
        for (const double rate : {-0.03, 0.03}) {
            const auto market = *make_pricing_context(*make_bsm_parameters(rate, 0.0, sigma), 100.0, start);
            const bool exercised = sign * rate > 0.0;
            const auto cash_result = engine.price_with_greeks(cash, market, {Greek::delta, Greek::gamma});
            const auto asset_result = engine.price_with_greeks(asset, market, {Greek::delta, Greek::gamma});
            REQUIRE(cash_result);
            REQUIRE(asset_result);
            CHECK(cash_result->price() == Catch::Approx(exercised ? std::exp(-rate) : 0.0));
            CHECK(asset_result->price() == (exercised ? 100.0 : 0.0));
            CHECK(*cash_result->get(Greek::delta) == 0.0);
            CHECK(*cash_result->get(Greek::gamma) == 0.0);
            CHECK(*asset_result->get(Greek::delta) == (exercised ? 1.0 : 0.0));
            CHECK(*asset_result->get(Greek::gamma) == 0.0);
        }
        const auto atm = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), 100.0, start);
        const auto cash_atm = engine.price_with_greeks(cash, atm, {Greek::delta, Greek::gamma});
        const auto asset_atm = engine.price_with_greeks(asset, atm, {Greek::delta, Greek::gamma});
        REQUIRE(cash_atm);
        REQUIRE(asset_atm);
        CHECK(cash_atm->price() == 0.5);
        CHECK(asset_atm->price() == 50.0);
        CHECK(*cash_atm->get(Greek::delta) == Catch::Approx(sign * detail::inverse_sqrt_two_pi / 100.0 / sigma).epsilon(1e-12));
        CHECK(*cash_atm->get(Greek::gamma) == Catch::Approx(-sign * 0.5 * detail::inverse_sqrt_two_pi / 100.0 / 100.0 / sigma).epsilon(1e-12));
        CHECK(*asset_atm->get(Greek::delta) == Catch::Approx(sign * detail::inverse_sqrt_two_pi / sigma).epsilon(1e-12));
        CHECK(*asset_atm->get(Greek::gamma) == Catch::Approx(sign * 0.5 * detail::inverse_sqrt_two_pi / 100.0 / sigma).epsilon(1e-12));
        // Independent 70-digit Decimal references at d=40, where double-precision normal_pdf is zero.
        const auto tail = *make_pricing_context(*make_bsm_parameters(40.0 * sigma, 0.0, sigma), 100.0, start);
        const auto cash_tail = engine.price_with_greeks(cash, tail, {Greek::delta, Greek::gamma});
        const auto asset_tail = engine.price_with_greeks(asset, tail, {Greek::gamma});
        REQUIRE(cash_tail);
        REQUIRE(asset_tail);
        CHECK(*cash_tail->get(Greek::delta) == Catch::Approx(sign * 1.4632702508383032e-150).epsilon(1e-11).margin(0.0));
        CHECK(*cash_tail->get(Greek::gamma) == Catch::Approx(-sign * 5.853081003353213e49).epsilon(1e-11));
        CHECK(*asset_tail->get(Greek::gamma) == Catch::Approx(-sign * 5.853081003353213e51).epsilon(1e-11));
        const auto subnormal = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 1e-310), 100.0, start);
        REQUIRE(engine.price(asset, subnormal));
        const auto finite_gamma = engine.price_with_greeks(asset, subnormal, {Greek::gamma});
        REQUIRE(finite_gamma);
        CHECK(*finite_gamma->get(Greek::gamma) == Catch::Approx(sign * 1.9947114020071634e307).epsilon(1e-11));
        const auto overflow = engine.price_with_greeks(asset, subnormal, {Greek::delta});
        REQUIRE_FALSE(overflow);
        CHECK(overflow.error().category == ErrorCategory::invalid_result);
    }
}

TEST_CASE("Digital expiry_date settlement uses strict strikes without smooth Greeks")
{
    struct Settlement {
        kiyosi::OptionType type;
        double spot;
        double cash;
        double asset;
    };
    // These exact settlement values are also checked against QuantLib payoff bindings.
    const std::array cases{
        Settlement{kiyosi::OptionType::call, 99, 0, 0}, Settlement{kiyosi::OptionType::call, 100, 0, 0},
        Settlement{kiyosi::OptionType::call, 101, 10, 101}, Settlement{kiyosi::OptionType::put, 99, 10, 99},
        Settlement{kiyosi::OptionType::put, 100, 0, 0}, Settlement{kiyosi::OptionType::put, 101, 0, 0}};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 6};
    for (const auto& item : cases) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), item.spot, expiry_date);
        const auto cash = *kiyosi::make_cash_or_nothing_option(item.type, 100, 10, expiry_date, expiry_date);
        const auto asset = *kiyosi::make_asset_or_nothing_option(item.type, 100, expiry_date, expiry_date);
        const auto check = [&](const auto& engine, const auto& option, double expected) {
            const auto result = engine.price_with_greeks(option, context, kiyosi::GreeksRequest{true});
            REQUIRE(result.has_value());

            CHECK(result->price() == expected);
            for (const auto& [name, measure] : measures)
                if (name != "price") CHECK_FALSE(result->has(measure));
        };
        const auto check_engine = [&](const auto& engine) {
            check(engine, cash, item.cash);
            check(engine, asset, item.asset);
        };
        check_engine(kiyosi::AnalyticDigitalEngine{});
        check_engine(kiyosi::QuadratureDigitalEngine{});
        check_engine(kiyosi::FiniteDifferenceDigitalEngine{});
    }
}
