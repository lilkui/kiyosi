#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <limits>

#include <kiyosi/kiyosi.hpp>
#include "pricing/detail/math.hpp"
#include "support/common.hpp"

TEST_CASE("Digital and binary barrier terminal payoffs share strict strike boundaries", "[binary-terminal-payoff]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    for (const auto type : {OptionType::call, OptionType::put}) {
        for (const double spot : {99.0, std::nextafter(100.0, 0.0), 100.0, std::nextafter(100.0, 101.0), 101.0}) {
            CAPTURE(type, spot);
            const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), spot, end);
            const bool exercised = type == OptionType::call ? spot > 100.0 : spot < 100.0;
            const auto check = [&](const auto& engine, const auto& option, double expected) {
                const auto price = engine.price(option, context);
                REQUIRE(price);
                CHECK(*price == expected);
                const auto result = engine.price_with_greeks(option, context, {Greek::delta});
                REQUIRE(result);
                CHECK(result->price() == expected);
                CHECK_FALSE(result->has(Greek::delta));
            };
            const auto cash = *make_cash_or_nothing_option(type, 100.0, 7.0, start, end);
            const auto asset = *make_asset_or_nothing_option(type, 100.0, start, end);
            const auto check_digital = [&](const auto& engine) {
                check(engine, cash, exercised ? 7.0 : 0.0);
                check(engine, asset, exercised ? spot : 0.0);
            };
            check_digital(AnalyticDigitalEngine{});
            check_digital(QuadratureDigitalEngine{});
            check_digital(FiniteDifferenceDigitalEngine{});
            for (const bool knock_in : {false, true}) {
                for (const bool touched : {false, true}) {
                    CAPTURE(knock_in, touched);
                    const BinaryBarrierTerms terms{
                        .option_type = type, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 50.0, .barrier_type = knock_in ? BarrierType::down_and_in : BarrierType::down_and_out, .touch_state = touched ? BarrierTouchState::touched : BarrierTouchState::untouched};
                    const bool pays = exercised && knock_in == touched;
                    check(AnalyticBinaryBarrierEngine{}, *make_cash_binary_barrier_option(terms, 7.0), pays ? 7.0 : 0.0);
                    check(AnalyticBinaryBarrierEngine{}, *make_asset_binary_barrier_option(terms), pays ? spot : 0.0);
                }
            }
        }
    }
}

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
                        CHECK(*price == Catch::Approx(expected).epsilon(0.0).margin(1e-10));
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

TEST_CASE("Digital CN startup preserves payoff bounds with one time interval", "[fd-rannacher]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 6};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto expiry = start + std::chrono::days{365};
    const auto engine = make_finite_difference_digital_engine({10'000, 1}).value();
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto cash = *make_cash_or_nothing_option(type, 100.5, 100.0, start, expiry);
        const auto asset = *make_asset_or_nothing_option(type, 100.5, start, expiry);
        const auto check = [&](const auto& option) {
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price > 40.0);
            CHECK(*price < 60.0);
            const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
            REQUIRE(joint);
            CHECK(joint->price() == *price);
            CHECK(joint->all_finite());
            const auto stable = make_finite_difference_digital_engine({10'000, 1, FiniteDifferenceScheme::implicit_euler}).value().price(option, context);
            REQUIRE(stable);
            CHECK(*stable > 40.0);
            CHECK(*stable < 60.0);
        };
        check(cash);
        check(asset);
    }
}

TEST_CASE("Digital damped CN preserves high-volatility payoff bounds", "[fd-rannacher]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 6};
    const auto volatile_context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 3.0), 100.0, start);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto cash = *make_cash_or_nothing_option(type, 110.0, 100.0, start, start + std::chrono::days{1825});
        const auto price = FiniteDifferenceDigitalEngine{}.price(cash, volatile_context);
        REQUIRE(price);
        CHECK(*price >= 0.0);
        CHECK(*price <= 100.0 * std::exp(-0.05 * 5.0));
    }
}

TEST_CASE("Digital damped CN still rejects oscillatory prices", "[fd-rannacher]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1};
    const auto expiry = start + std::chrono::days{365};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 10.0), 20.0, start);
    const auto option = *make_cash_or_nothing_option(OptionType::call, 100.5, 100.0, start, expiry);
    const auto engine = make_finite_difference_digital_engine({1000, 3, FiniteDifferenceScheme::crank_nicolson, 400.0}).value();
    const auto price = engine.price(option, context);
    REQUIRE_FALSE(price);
    CHECK(price.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("Digital damped CN bounds allow known positive and negative carry truncation", "[fd-rannacher]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 6};
    const auto expiry = start + std::chrono::days{365};
    for (const double rate : {-0.05, 0.05}) {
        const double dividend = 0.4 * rate;
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, dividend, 0.2), 100.0, start);
        for (const int steps : {1, 200}) {
            const auto engine = make_finite_difference_digital_engine({800, steps}).value();
            const auto check = [&](const auto& option, double upper, double delta) {
                const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                CAPTURE(rate, steps);
                REQUIRE(actual);
                CHECK(actual->price() >= 0.0);
                CHECK(actual->price() <= upper);
                CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(upper, 1e-8));
                CHECK_THAT(test::greek_value(*actual, Greek::delta), Catch::Matchers::WithinAbs(delta, 0.001));
                CHECK_THAT(test::greek_value(*actual, Greek::gamma), Catch::Matchers::WithinAbs(0.0, 1e-8));
            };
            check(*make_cash_or_nothing_option(OptionType::call, 1.0, 100.0, start, expiry), 100.0 * std::exp(-rate), 0.0);
            check(*make_asset_or_nothing_option(OptionType::call, 1.0, start, expiry), 100.0 * std::exp(-dividend), std::exp(-dividend));
        }
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
            for (std::size_t i = 0; i < kiyosi::greek_count; ++i)
                CHECK_FALSE(result->has(static_cast<kiyosi::Greek>(i)));
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
