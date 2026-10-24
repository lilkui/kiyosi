#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"
#include "pricing/detail/binomial.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::greek_value;

TEST_CASE("European binomial expectations match full rollback", "[binomial-expectation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const int steps : {1, 2, 3, 17, 127, 128, 129, 255, 256, 257, 512, 1000, 1024})
        for (const double rate : {-0.2, 0.04, 0.2})
            for (const auto type : {OptionType::call, OptionType::put}) {
                CAPTURE(steps, rate, type);
                const double up = std::exp(0.2 / std::sqrt(steps));
                const double down = 1.0 / up;
                const double probability = std::expm1(rate / steps + 0.2 / std::sqrt(steps)) / std::expm1(0.4 / std::sqrt(steps));
                const double discount = std::exp(-rate / steps);
                const double sign = type == OptionType::call ? 1.0 : -1.0;
                std::vector<double> values(static_cast<std::size_t>(steps) + 1);
                double spot = 100.0 * std::pow(down, steps);
                for (auto& value : values) {
                    value = std::max(sign * (spot - 100.0), 0.0);
                    spot *= up * up;
                }
                std::array<double, 3> second{};
                std::array<double, 2> first{};
                for (int level = steps; level > 0; --level) {
                    if (level == 2) second = {values[0], values[1], values[2]};
                    if (level == 1) first = {values[0], values[1]};
                    REQUIRE(detail::binomial_rollback<false>(std::span{values}.first(level + 1), discount, probability, 0.0, 0.0, 0.0, 0.0));
                }
                const auto option = make_european_option(type, 100.0, start, end).value();
                const auto context = make_pricing_context(make_bsm_parameters(rate, 0.0, 0.2).value(), 100.0, start).value();
                const auto engine = make_cox_ross_rubinstein_vanilla_engine({steps}).value();
                const auto price = engine.price(option, context);
                REQUIRE(price);
                CHECK_THAT(*price, Catch::Matchers::WithinAbs(values[0], 1e-10));
                for (const auto request : {GreeksRequest{Greek::delta}, GreeksRequest{Greek::gamma}, GreeksRequest{Greek::delta, Greek::gamma}}) {
                    const auto greeks = engine.price_with_greeks(option, context, request);
                    REQUIRE(greeks);
                    CHECK(greeks->price() == *price);
                    if (request.has(Greek::delta))
                        CHECK_THAT(greek_value(*greeks, Greek::delta), Catch::Matchers::WithinAbs((first[1] - first[0]) / (100.0 * (up - down)), 1e-10));
                    if (request.has(Greek::gamma) && steps >= 2) {
                        const double gamma = ((second[2] - second[1]) / (100.0 * (up * up - 1.0)) - (second[1] - second[0]) / (100.0 * (1.0 - down * down))) / (50.0 * (up * up - down * down));
                        CHECK_THAT(greek_value(*greeks, Greek::gamma), Catch::Matchers::WithinAbs(gamma, 1e-10));
                    }
                }
            }
}

TEST_CASE("European binomial expectations retain deterministic endpoints and overflow errors", "[binomial-expectation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto engine = make_cox_ross_rubinstein_vanilla_engine({1024}).value();
    for (const double rate : {-6.4, 6.4}) {
        const auto context = make_pricing_context(make_bsm_parameters(rate, 0.0, 0.2).value(), 100.0, start).value();
        for (const auto type : {OptionType::call, OptionType::put}) {
            const auto option = make_european_option(type, 100.0, start, end).value();
            const double sign = type == OptionType::call ? 1.0 : -1.0;
            const double terminal = 100.0 * std::exp(rate);
            const double expected = std::max(sign * (terminal - 100.0), 0.0) * std::exp(-rate);
            const auto actual = engine.price(option, context);
            REQUIRE(actual);
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(expected, std::max(1e-10, expected * 1e-11)));
        }
    }
    const auto context = make_pricing_context(make_bsm_parameters(-1000.0, -1000.0, 0.2).value(), 100.0, start).value();
    const auto price = engine.price(make_european_option(OptionType::put, 100.0, start, end).value(), context);
    REQUIRE_FALSE(price);
    CHECK(price.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("European binomial expectations recover underflowed tail weights", "[binomial-expectation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const int steps = 2048;
    const double spot = 1e280;
    const double strike = spot * std::exp(8.0);
    const double log_up = 0.2 / std::sqrt(steps);
    const double up = std::exp(log_up), down = 1.0 / up;
    const double probability = std::expm1(0.03 / steps + log_up) / std::expm1(2.0 * log_up);
    const double discount = std::exp(-0.04 / steps);
    std::vector<double> values(steps + 1);
    double node_spot = spot * std::pow(down, steps);
    for (auto& value : values) {
        value = std::max(node_spot - strike, 0.0);
        node_spot *= up * up;
    }
    for (int level = steps; level > 0; --level)
        REQUIRE(detail::binomial_rollback<false>(std::span{values}.first(level + 1), discount, probability, 0.0, 0.0, 0.0, 0.0));
    REQUIRE(values[0] > 0.0);
    const auto option = *make_european_option(OptionType::call, strike, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), spot, start);
    const auto engine = *make_cox_ross_rubinstein_vanilla_engine({steps});
    const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
    const auto price = engine.price(option, context);
    REQUIRE(actual);
    REQUIRE(price);
    CHECK_THAT(*price, Catch::Matchers::WithinRel(values[0], 1e-9));
    CHECK(actual->price() == *price);
}

TEST_CASE("American binomial calls preserve European values only without early exercise", "[binomial-expectation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto american = *make_american_option(OptionType::call, 100.0, start, end);
    const auto european = *make_european_option(OptionType::call, 100.0, start, end);
    for (const auto [rate, dividend] : {std::pair{0.0, 0.0}, {0.04, 0.0}, {0.04, -0.01}, {-0.04, 0.0}, {0.04, 0.08}})
        for (const double spot : {100.0, 200.0})
            for (const int steps : {255, 256, 257, 511, 512, 1024}) {
                CAPTURE(rate, dividend, spot, steps);
                const auto context = *make_pricing_context(*make_bsm_parameters(rate, dividend, 0.2), spot, start);
                const auto engine = *make_cox_ross_rubinstein_vanilla_engine({steps});
                const auto actual = engine.price_with_greeks(american, context, {Greek::delta, Greek::gamma});
                const auto expected = engine.price_with_greeks(european, context, {Greek::delta, Greek::gamma});
                const auto price = engine.price(american, context);
                REQUIRE(actual);
                REQUIRE(expected);
                REQUIRE(price);
                CHECK(actual->price() == *price);
                CHECK(*price >= std::max(spot - 100.0, 0.0));
                if (rate >= 0.0 && dividend <= 0.0) {
                    CHECK_THAT(*price, Catch::Matchers::WithinAbs(expected->price(), 1e-10));
                    for (const auto greek : {Greek::delta, Greek::gamma})
                        CHECK_THAT(greek_value(*actual, greek), Catch::Matchers::WithinAbs(greek_value(*expected, greek), 1e-10));
                } else if (spot == 200.0) {
                    CHECK(*price > expected->price() + 1.0);
                    CHECK(greek_value(*actual, Greek::delta) == 1.0);
                    CHECK(greek_value(*actual, Greek::gamma) == 0.0);
                }
            }
}

TEST_CASE("Binomial immediate exercise returns payoff Greeks", "[binomial-exercise-greeks]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const auto type : {OptionType::put, OptionType::call}) {
        const bool put = type == OptionType::put;
        const double spot = put ? 88.0 : 100.0;
        const auto parameters = *make_bsm_parameters(put ? 0.15 : 0.0, put ? 0.0 : 0.15, 0.2);
        const auto option = *make_american_option(type, put ? 100.0 : 88.0, start, end);
        const auto context = *make_pricing_context(parameters, spot, start);
        for (const int steps : {1, 2, 3, 4, 5, 6, 7, 8, 10, 255, 256, 257}) {
            CAPTURE(type, steps);
            const auto engine = make_cox_ross_rubinstein_vanilla_engine({steps}).value();
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(result->price() == 12.0);
            CHECK(greek_value(*result, Greek::delta) == (put ? -1.0 : 1.0));
            CHECK(greek_value(*result, Greek::gamma) == 0.0);
            const auto high = engine.price(option, *make_pricing_context(parameters, spot + 0.001, start));
            const auto low = engine.price(option, *make_pricing_context(parameters, spot - 0.001, start));
            REQUIRE(high);
            REQUIRE(low);
            CHECK_THAT((*high - *low) / 0.002,
                       Catch::Matchers::WithinAbs(greek_value(*result, Greek::delta), 1e-10));
        }
    }
}

TEST_CASE("Binomial speed tracks analytic sensitivity with default shifts", "[binomial-speed]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto engine = make_cox_ross_rubinstein_vanilla_engine({1000}).value();
    for (const double dividend : {0.0, 0.01}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, dividend, 0.2), 100.0,
                                                   start_of_day(start) + std::chrono::hours{12});
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(dividend, type);
            const auto european = *make_european_option(type, 100.0, start, end);
            const auto expected = AnalyticVanillaEngine{}.price_with_greeks(european, context, {Greek::speed});
            REQUIRE(expected);
            const auto check = [&](const auto& option) {
                const auto actual = engine.price_with_greeks(option, context, {Greek::speed});
                const auto price = engine.price(option, context);
                REQUIRE(actual);
                REQUIRE(price);
                CHECK(actual->price() == *price);
                const double reference = greek_value(*expected, Greek::speed);
                CHECK_THAT(greek_value(*actual, Greek::speed),
                           Catch::Matchers::WithinAbs(reference, std::abs(reference) * 0.02));
                CHECK_FALSE(actual->has(Greek::gamma));
            };
            check(european);
            if (type == OptionType::call && dividend == 0.0)
                check(*make_american_option(type, 100.0, start, end));
        }
    }
}

TEST_CASE("Binomial mixed Greeks track analytic sensitivities", "[binomial-mixed-greeks]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const GreeksRequest request{Greek::vanna, Greek::zomma, Greek::charm, Greek::color};
    const auto engine = make_cox_ross_rubinstein_vanilla_engine({1000}).value();
    for (const double dividend : {0.0, 0.01}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, dividend, 0.2), 100.0,
                                                   start_of_day(start) + std::chrono::hours{12});
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(dividend, type);
            const auto european = *make_european_option(type, 100.0, start, end);
            const auto expected = AnalyticVanillaEngine{}.price_with_greeks(european, context, request);
            REQUIRE(expected);
            const auto check = [&](const auto& option) {
                const auto actual = engine.price_with_greeks(option, context, request);
                const auto price = engine.price(option, context);
                REQUIRE(actual);
                REQUIRE(price);
                CHECK(actual->price() == *price);
                for (const auto greek : {Greek::vanna, Greek::zomma, Greek::charm, Greek::color}) {
                    CAPTURE(greek);
                    const double reference = greek_value(*expected, greek);
                    CHECK_THAT(greek_value(*actual, greek),
                               Catch::Matchers::WithinAbs(reference, std::abs(reference) * 0.02 + 1e-7));
                }
                CHECK_FALSE(actual->has(Greek::delta));
                CHECK_FALSE(actual->has(Greek::gamma));
            };
            check(european);
            if (type == OptionType::call && dividend == 0.0)
                check(*make_american_option(type, 100.0, start, end));
        }
    }
}

TEST_CASE("Binomial rejects underflowed asset trees")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto engine = kiyosi::make_cox_ross_rubinstein_vanilla_engine({1000}).value();
    for (const double volatility : {23.0, 25.0}) {
        const auto context = *kiyosi::make_pricing_context(
            *kiyosi::make_bsm_parameters(0.05, 0.02, volatility), 100.0, start);
        for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
            const auto european = *kiyosi::make_european_option(type, 100.0, start, end);
            const auto american = *kiyosi::make_american_option(type, 100.0, start, end);
            for (const auto& price : {engine.price(european, context), engine.price(american, context)}) {
                REQUIRE_FALSE(price);
                CHECK(price.error().category == kiyosi::ErrorCategory::invalid_result);
            }
        }
    }
}

TEST_CASE("Binomial American engine prices expiry_date and validates steps")
{
    const auto expiry_date = day(2025, 1, 1);
    const auto option = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, expiry_date, expiry_date);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 90.0, expiry_date);
    const kiyosi::CoxRossRubinsteinVanillaEngine engine;

    const auto priced = engine.price(option, context);
    REQUIRE(priced.has_value());
    CHECK(*priced == 10.0);

    const auto invalid = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{0});
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto too_many = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{1'000'001});
    REQUIRE_FALSE(too_many.has_value());
    CHECK(too_many.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("Binomial American engine supplements gamma below two steps")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto option = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);

    const auto result = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{1}).value().price_with_greeks(option, context, kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma});
    REQUIRE(result.has_value());
    CHECK(std::isfinite(result->price()));
    CHECK(result->has(kiyosi::Greek::delta));
    CHECK(result->has(kiyosi::Greek::gamma));
    const auto gamma = result->get(kiyosi::Greek::gamma);
    REQUIRE(gamma.has_value());
    REQUIRE(gamma->has_value());
    CHECK_THAT(gamma.value().value_or(std::numeric_limits<double>::quiet_NaN()),
               Catch::Matchers::WithinAbs(0.0, 1e-7));
}

TEST_CASE("Binomial American engine exercises puts and converges to European calls")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 90.0, valuation);
    const auto put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto european_put_option = *kiyosi::make_european_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto american_call_option = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{400}).value();

    const auto american_put = engine.price(put, context);
    const auto european_put = kiyosi::AnalyticVanillaEngine{}.price(european_put_option, context);
    REQUIRE(american_put.has_value());
    REQUIRE(european_put.has_value());
    CHECK(*american_put >
          *european_put);

    const auto at_the_money_context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto american_call = engine.price_with_greeks(american_call_option, at_the_money_context, kiyosi::GreeksRequest{kiyosi::Greek::delta, kiyosi::Greek::gamma});
    const auto european_call = kiyosi::AnalyticVanillaEngine{}.price(call, at_the_money_context);
    REQUIRE(american_call.has_value());
    REQUIRE(european_call.has_value());
    CHECK_THAT(american_call->price(),
               WithinAbs(*european_call, 0.02));
    CHECK(std::isfinite(greek_value(*american_call, kiyosi::Greek::delta)));
    CHECK(std::isfinite(greek_value(*american_call, kiyosi::Greek::gamma)));

    const auto high_resolution = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{1200}).value().price(american_call_option, at_the_money_context);
    REQUIRE(high_resolution.has_value());
    CHECK_THAT(*high_resolution,
               WithinAbs(*european_call, 0.01));
}

TEST_CASE("Binomial American call and put values are symmetric at zero carry")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_american_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_cox_ross_rubinstein_vanilla_engine(kiyosi::BinomialSettings{200}).value();

    const auto call_result = engine.price(call, context);
    const auto put_result = engine.price(put, context);
    REQUIRE(call_result.has_value());
    REQUIRE(put_result.has_value());
    CHECK(std::abs(*call_result -
                   *put_result) <= 1e-12);
}

} // namespace
