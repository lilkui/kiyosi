#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>

#include "support/reference_harness.hpp"
#include "pricing/detail/math.hpp"

using kiyosi::test::measures;

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
