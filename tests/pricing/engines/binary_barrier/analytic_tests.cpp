#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <cmath>
#include <limits>
#include <numbers>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {

using kiyosi::test::day;

}

TEST_CASE("Binary barrier branches preserve in-out parity", "[binary-barrier-branches]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start);
    const AnalyticBinaryBarrierEngine engine;
    for (const bool up : {false, true})
        for (const auto type : {OptionType::call, OptionType::put})
            for (const double strike : {70.0, 80.0, 100.0, 120.0, 130.0})
                for (const bool asset : {false, true}) {
                    CAPTURE(up, type, strike, asset);
                    const BinaryBarrierTerms in_terms{
                        .option_type = type, .strike = strike, .effective_date = start, .expiry_date = end, .barrier_level = up ? 120.0 : 80.0, .barrier_type = up ? BarrierType::up_and_in : BarrierType::down_and_in};
                    auto out_terms = in_terms;
                    out_terms.barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out;
                    const auto knock_in = asset ? make_asset_binary_barrier_option(in_terms) : make_cash_binary_barrier_option(in_terms, 10.0);
                    const auto knock_out = asset ? make_asset_binary_barrier_option(out_terms) : make_cash_binary_barrier_option(out_terms, 10.0);
                    REQUIRE(knock_in);
                    REQUIRE(knock_out);
                    const auto in_price = engine.price(*knock_in, context), out_price = engine.price(*knock_out, context);
                    const auto vanilla = asset ? AnalyticDigitalEngine{}.price(*make_asset_or_nothing_option(type, strike, start, end), context)
                                               : AnalyticDigitalEngine{}.price(*make_cash_or_nothing_option(type, strike, 10.0, start, end), context);
                    REQUIRE(in_price);
                    REQUIRE(out_price);
                    REQUIRE(vanilla);
                    CHECK_THAT(*in_price + *out_price, Catch::Matchers::WithinAbs(*vanilla, 1e-10));
                }
}

TEST_CASE("Scheduled analytic barriers preserve prices when the shifted level overflows", "[scheduled-barrier-scale]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const std::vector observations{day(2025, 4, 1), day(2025, 7, 1), day(2025, 10, 1), end};
    const auto parameters = *make_bsm_parameters(0.0, 0.0, 1.0);
    const AnalyticBinaryBarrierEngine binary_engine;
    for (const double scale : {1.0, 1e306}) {
        CAPTURE(scale);
        const auto context = *make_pricing_context(parameters, 100.0 * scale, start);
        const auto touch = *make_cash_one_touch_up(start, end, 170.0 * scale, 1.0, SettlementTiming::at_expiry, ObservationMode::scheduled, observations);
        const auto no_touch = *make_cash_no_touch_up(start, end, 170.0 * scale, 1.0, ObservationMode::scheduled, observations);
        const auto touched = binary_engine.price(touch, context);
        const auto untouched = binary_engine.price(no_touch, context);
        REQUIRE(touched);
        REQUIRE(untouched);
        CHECK_THAT(*touched, Catch::Matchers::WithinAbs(0.25739250301126404, 1e-12));
        CHECK_THAT(*untouched, Catch::Matchers::WithinAbs(0.742607496988736, 1e-12));
    }
    for (const auto type : {OptionType::call, OptionType::put})
        for (const auto barrier_type : {BarrierType::up_and_in, BarrierType::up_and_out}) {
            CAPTURE(type, barrier_type);
            const auto prices = [&](double scale) {
                const auto context = *make_pricing_context(parameters, 100.0 * scale, start);
                const auto vanilla = *make_barrier_option({.option_type = type, .strike = 175.0 * scale, .effective_date = start, .expiry_date = end, .barrier_level = 170.0 * scale, .barrier_type = barrier_type, .observation_mode = ObservationMode::scheduled, .observation_dates = observations});
                const auto binary = *make_cash_binary_barrier_option({.option_type = type, .strike = 175.0 * scale, .effective_date = start, .expiry_date = end, .barrier_level = 170.0 * scale, .barrier_type = barrier_type, .observation_mode = ObservationMode::scheduled, .observation_dates = observations}, 1.0);
                const auto vanilla_price = AnalyticBarrierEngine{}.price(vanilla, context);
                const auto binary_price = binary_engine.price(binary, context);
                REQUIRE(vanilla_price);
                REQUIRE(binary_price);
                return std::pair{*vanilla_price / scale, *binary_price};
            };
            const auto reference = prices(1.0), scaled = prices(1e306);
            CHECK_THAT(scaled.first, Catch::Matchers::WithinAbs(reference.first, 1e-10));
            CHECK_THAT(scaled.second, Catch::Matchers::WithinAbs(reference.second, 1e-12));
        }
}

TEST_CASE("First-hit payments retain scaled far-tail prices", "[first-hit-scale]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.0, 0.0, 0.2);
    const auto context = *make_pricing_context(parameters, 100.0, start);
    for (const bool up : {false, true})
        for (const double amount : {1e200, 1e300}) {
            CAPTURE(up, amount);
            const double ratio = std::exp(up ? 8.0 : -8.0);
            const double barrier = 100.0 * ratio;
            // High-precision first-passage values at log-distance 8 and volatility 20%.
            const double expected = (up ? 1.3325295301744355e-51 : 3.9722145459424451e-48) * (amount / 1e300);
            const auto asset_context = *make_pricing_context(parameters, amount / ratio, start);
            const auto check = [&](const auto& engine, const auto& option, const auto& market) {
                const auto price = engine.price(option, market);
                REQUIRE(price);
                CHECK_THAT(*price, Catch::Matchers::WithinRel(expected, 1e-11));
                const auto joint = engine.price_with_greeks(option, market, {Greek::delta});
                REQUIRE(joint);
                CHECK(joint->price() == *price);
            };
            for (const auto timing : {SettlementTiming::at_hit, SettlementTiming::at_expiry}) {
                CAPTURE(timing);
                const auto cash = *(up ? make_cash_one_touch_up(start, end, barrier, amount, timing)
                                       : make_cash_one_touch_down(start, end, barrier, amount, timing));
                const auto asset = *(up ? make_asset_one_touch_up(start, end, amount, timing)
                                        : make_asset_one_touch_down(start, end, amount, timing));
                const auto rebate = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put,
                                                          .strike = up ? 2.0 * barrier : 0.5 * barrier,
                                                          .effective_date = start,
                                                          .expiry_date = end,
                                                          .barrier_level = barrier,
                                                          .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out,
                                                          .rebate = amount,
                                                          .rebate_timing = timing == SettlementTiming::at_hit ? RebateTiming::at_hit : RebateTiming::at_expiry});
                check(AnalyticBinaryBarrierEngine{}, cash, context);
                check(AnalyticBinaryBarrierEngine{}, asset, asset_context);
                check(AnalyticBarrierEngine{}, rebate, context);
            }
        }
}

TEST_CASE("First-hit payments scale extreme discounts", "[first-hit-scale]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const bool up : {false, true}) {
        CAPTURE(up);
        const double spot = up ? 1e-200 : 1e200;
        const double barrier = up ? 1e200 : 1e-200;
        const double rate = up ? -1000.0 : 1000.0;
        const double amount = spot;
        const double distance = std::abs(std::log(barrier) - std::log(spot));
        const double expected = std::exp(std::log(amount) - rate * (distance / 1000.0));
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, 2.0 * rate, 1e-200), spot, start);
        const auto cash = *(up ? make_cash_one_touch_up(start, end, barrier, amount, SettlementTiming::at_hit)
                               : make_cash_one_touch_down(start, end, barrier, amount, SettlementTiming::at_hit));
        const auto rebate = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put,
                                                  .strike = up ? 2.0 * barrier : 0.5 * barrier,
                                                  .effective_date = start,
                                                  .expiry_date = end,
                                                  .barrier_level = barrier,
                                                  .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out,
                                                  .rebate = amount,
                                                  .rebate_timing = RebateTiming::at_hit});
        const auto touch_price = AnalyticBinaryBarrierEngine{}.price(cash, context);
        const auto rebate_price = AnalyticBarrierEngine{}.price(rebate, context);
        REQUIRE(touch_price);
        REQUIRE(rebate_price);
        CHECK_THAT(*touch_price, Catch::Matchers::WithinRel(expected, 1e-11));
        CHECK_THAT(*rebate_price, Catch::Matchers::WithinRel(expected, 1e-11));
    }
}

TEST_CASE("Binary barrier prices survive volatility variance underflow", "[binary-barrier-underflow]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const AnalyticBinaryBarrierEngine engine;
    for (const double sigma : {1e-160, 1e-200})
        for (const double rate : {0.0, 0.05})
            for (const double spot : {90.0, 100.0, 110.0})
                for (const auto direction : {OptionType::call, OptionType::put})
                    for (const auto type : {BarrierType::down_and_in, BarrierType::down_and_out,
                                            BarrierType::up_and_in, BarrierType::up_and_out})
                        for (const bool asset : {false, true}) {
                            CAPTURE(sigma, rate, spot, direction, type, asset);
                            const bool up = type == BarrierType::up_and_in || type == BarrierType::up_and_out;
                            const bool knock_in = type == BarrierType::up_and_in || type == BarrierType::down_and_in;
                            const BinaryBarrierTerms terms{.option_type = direction, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 120.0 : 80.0, .barrier_type = type};
                            const auto option = *(asset ? make_asset_binary_barrier_option(terms) : make_cash_binary_barrier_option(terms, 10.0));
                            const auto context = *make_pricing_context(*make_bsm_parameters(rate, rate, sigma), spot, start);
                            const double probability = spot == 100.0 ? 0.5 : ((direction == OptionType::call ? spot > 100.0 : spot < 100.0) ? 1.0 : 0.0);
                            const double expected = knock_in ? 0.0 : (asset ? spot : 10.0) * std::exp(-rate) * probability;
                            const auto price = engine.price(option, context);
                            REQUIRE(price);
                            CHECK_THAT(*price, Catch::Matchers::WithinAbs(expected, 1e-11));
                            const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
                            REQUIRE(joint);
                            CHECK(joint->price() == *price);
                        }
}

TEST_CASE("Binary barriers preserve small-volatility prices with nonzero carry", "[barrier-carry]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const double sigma : {1e-160, 1e-200})
        for (const double carry : {-0.3, -0.02, 0.02, 0.3})
            for (const bool up : {false, true})
                for (const bool knock_in : {false, true})
                    for (const auto direction : {OptionType::call, OptionType::put})
                        for (const bool asset : {false, true}) {
                            CAPTURE(sigma, carry, up, knock_in, direction, asset);
                            const double terminal = 100.0 * std::exp(carry);
                            const bool touched = up ? terminal >= 120.0 : terminal <= 80.0;
                            const bool exercised = direction == OptionType::call ? terminal > 100.0 : terminal < 100.0;
                            const double expected = knock_in == touched && exercised ? (asset ? terminal : 10.0) * std::exp(-0.03) : 0.0;
                            const auto context = *make_pricing_context(*make_bsm_parameters(0.03, 0.03 - carry, sigma), 100.0, start);
                            const BinaryBarrierTerms terms{.option_type = direction, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 120.0 : 80.0, .barrier_type = up ? (knock_in ? BarrierType::up_and_in : BarrierType::up_and_out) : (knock_in ? BarrierType::down_and_in : BarrierType::down_and_out)};
                            const auto option = *(asset ? make_asset_binary_barrier_option(terms) : make_cash_binary_barrier_option(terms, 10.0));
                            const auto actual = AnalyticBinaryBarrierEngine{}.price(option, context);
                            REQUIRE(actual);
                            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(expected, 1e-11));
                        }
}

TEST_CASE("Reflected barrier tails survive overflowing exponents", "[barrier-carry]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const double carry = std::numbers::ln2;
    for (const double sigma : {1e-160, 1e-200}) {
        CAPTURE(sigma);
        const auto context = *make_pricing_context(*make_bsm_parameters(carry, 0.0, sigma), 0.5, start);
        const auto option = *make_cash_binary_barrier_option(
            {.option_type = OptionType::put, .strike = 1.0, .effective_date = start, .expiry_date = end, .barrier_level = 1.0, .barrier_type = BarrierType::up_and_in}, 1e300);
        const auto actual = AnalyticBinaryBarrierEngine{}.price(option, context);
        REQUIRE(actual);
        const double expected = (1e300 * sigma) / (4.0 * carry * std::sqrt(2.0 * std::numbers::pi));
        CHECK_THAT(*actual, Catch::Matchers::WithinRel(expected, 1e-12));
    }
}

TEST_CASE("Touch prices survive volatility variance underflow", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const AnalyticBinaryBarrierEngine engine;
    for (const bool up : {true, false}) {
        const double barrier = up ? 101.0 : 99.0;
        for (const double sigma : {1e-160, 1e-200, std::numeric_limits<double>::denorm_min()}) {
            for (const bool crossing : {false, true}) {
                const double dividend = crossing ? (up ? 0.0 : 0.1) : 0.05;
                const auto context = *make_pricing_context(*make_bsm_parameters(0.05, dividend, sigma), 100.0, start);
                const double hit_time = crossing ? std::log1p((barrier - 100.0) / 100.0) / (0.05 - dividend) : 0.0;
                for (const auto timing : {SettlementTiming::at_hit, SettlementTiming::at_expiry}) {
                    CAPTURE(up, sigma, crossing, timing);
                    const auto cash = *(up ? make_cash_one_touch_up : make_cash_one_touch_down)(start, end, barrier, 1.0, timing, ObservationMode::continuous, {}, std::nullopt);
                    const auto asset = *(up ? make_asset_one_touch_up : make_asset_one_touch_down)(start, end, barrier, timing, ObservationMode::continuous, {}, std::nullopt);
                    const double discount = std::exp(-0.05 * (timing == SettlementTiming::at_hit ? hit_time : 1.0));
                    const auto cash_price = engine.price(cash, context), asset_price = engine.price(asset, context);
                    REQUIRE(cash_price);
                    REQUIRE(asset_price);
                    CHECK_THAT(*cash_price, Catch::Matchers::WithinAbs(crossing ? discount : 0.0, 1e-12));
                    CHECK_THAT(*asset_price, Catch::Matchers::WithinAbs(crossing ? (timing == SettlementTiming::at_hit ? barrier * discount : 100.0 * std::exp(-dividend)) : 0.0, 1e-10));
                }
                const auto cash_no_touch = *(up ? make_cash_no_touch_up : make_cash_no_touch_down)(start, end, barrier, 1.0, ObservationMode::continuous, {}, std::nullopt);
                const auto asset_no_touch = *(up ? make_asset_no_touch_up : make_asset_no_touch_down)(start, end, barrier, ObservationMode::continuous, {}, std::nullopt);
                const auto cash_price = engine.price(cash_no_touch, context), asset_price = engine.price(asset_no_touch, context);
                REQUIRE(cash_price);
                REQUIRE(asset_price);
                CHECK_THAT(*cash_price, Catch::Matchers::WithinAbs(crossing ? 0.0 : std::exp(-0.05), 1e-12));
                CHECK_THAT(*asset_price, Catch::Matchers::WithinAbs(crossing ? 0.0 : 100.0 * std::exp(-dividend), 1e-10));
            }
        }
        const double drift = std::log1p((barrier - 100.0) / 100.0);
        const auto context = *make_pricing_context(*make_bsm_parameters(0.0, -drift, 1e-200), 100.0, start);
        const auto boundary = *(up ? make_cash_one_touch_up : make_cash_one_touch_down)(start, end, barrier, 1.0, SettlementTiming::at_expiry, ObservationMode::continuous, {}, std::nullopt);
        const auto price = engine.price(boundary, context);
        REQUIRE(price);
        CHECK(*price == 0.5);
    }
}

TEST_CASE("Scheduled one-touch expires at its final fixing", "[audit-fixes]")
{
    const auto effective = day(2025, 1, 1);
    const auto fixing = day(2025, 1, 2);
    const auto expiry = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto touch = *kiyosi::make_cash_one_touch_up(
        effective, expiry, 120.0, 10.0, kiyosi::SettlementTiming::at_expiry,
        kiyosi::ObservationMode::scheduled, {fixing}, kiyosi::BarrierTouchState::untouched);
    for (const double spot : {100.0, 120.0}) {
        const auto context = *kiyosi::make_pricing_context(parameters, spot, fixing);
        const auto price = kiyosi::AnalyticBinaryBarrierEngine{}.price(touch, context);
        REQUIRE(price);
        CHECK(*price == (spot < 120.0 ? 0.0 : 10.0 * std::exp(-0.05 * 364.0 / 365.0)));
    }
}

TEST_CASE("Asset one-touch current hits settle at the observed spot", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto event = day(2025, 1, 2);
    const auto end = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const kiyosi::AnalyticBinaryBarrierEngine engine;
    for (const bool up : {true, false}) {
        const auto factory = up ? kiyosi::make_asset_one_touch_up : kiyosi::make_asset_one_touch_down;
        const double spot = up ? 120.0 : 80.0;
        const double barrier = up ? 110.0 : 90.0;
        for (const auto valuation : {event, end}) {
            const auto context = *kiyosi::make_pricing_context(parameters, spot, valuation);
            for (const auto mode : {kiyosi::ObservationMode::scheduled, kiyosi::ObservationMode::continuous}) {
                CAPTURE(up, valuation, mode);
                const auto observations = mode == kiyosi::ObservationMode::scheduled
                                              ? std::vector<kiyosi::Date>{valuation}
                                              : std::vector<kiyosi::Date>{};
                const auto effective = mode == kiyosi::ObservationMode::continuous ? valuation : start;
                const auto immediate = *factory(effective, end, barrier, kiyosi::SettlementTiming::at_hit,
                                                mode, observations, std::nullopt);
                const auto deferred = *factory(effective, end, barrier, kiyosi::SettlementTiming::at_expiry,
                                               mode, observations, std::nullopt);
                const auto price = engine.price(immediate, context);
                const auto deferred_price = engine.price(deferred, context);
                REQUIRE(price);
                REQUIRE(deferred_price);
                CHECK(*price == spot);
                CHECK_THAT(*deferred_price, Catch::Matchers::WithinAbs(
                                                spot * std::exp(-0.02 * (end - valuation).count() / 365.0), 1e-12));
            }
        }
        const auto already_paid = *factory(start, end, barrier, kiyosi::SettlementTiming::at_hit,
                                           kiyosi::ObservationMode::scheduled, {start, event}, kiyosi::BarrierTouchState::touched);
        const auto result = engine.price(already_paid, *kiyosi::make_pricing_context(parameters, spot, event));
        REQUIRE(result);
        CHECK(*result == 0.0);
    }
}

TEST_CASE("Binary barriers expose observation intervals")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto observation_dates = std::vector<kiyosi::Date>{valuation + std::chrono::days{30},
                                                             valuation + std::chrono::days{180}};
    const auto binary = kiyosi::make_cash_binary_barrier_option(
        {.option_type = kiyosi::OptionType::call,
         .strike = 100.0,
         .effective_date = valuation,
         .expiry_date = expiry_date,
         .barrier_level = 90.0,
         .barrier_type = kiyosi::BarrierType::down_and_in,
         .observation_mode = kiyosi::ObservationMode::scheduled,
         .observation_dates = observation_dates},
        10.0);
    REQUIRE(binary);
    CHECK_THAT(binary->mean_observation_year_fraction(),
               Catch::Matchers::WithinAbs(180.0 / 365.0 / 2.0, 1e-12));
}

TEST_CASE("Settled binary barriers preserve extreme digital prices", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto effective = day(2024, 12, 31);
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const AnalyticBinaryBarrierEngine engine;
    const AnalyticDigitalEngine digital;
    for (const double volatility : {1e155, 0.5}) {
        const bool extreme_volatility = volatility > 1.0;
        const double spot = extreme_volatility ? 100.0 : 1.0;
        const double strike = extreme_volatility ? 100.0 : 1e9;
        const double payout = extreme_volatility ? 1.0 : 1e305;
        const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, volatility), spot, valuation);
        for (const auto type : {OptionType::call, OptionType::put}) {
            for (const bool asset : {false, true}) {
                const auto reference = asset
                                           ? digital.price(*make_asset_or_nothing_option(type, strike, effective, expiry), context)
                                           : digital.price(*make_cash_or_nothing_option(type, strike, payout, effective, expiry), context);
                REQUIRE(reference);
                if (extreme_volatility)
                    CHECK(*reference == (asset ? (type == OptionType::call ? spot : 0.0)
                                               : (type == OptionType::call ? 0.0 : payout)));
                else if (!asset && type == OptionType::call)
                    CHECK_THAT(*reference, Catch::Matchers::WithinRel(2.8067512790206434e-75, 1e-12));
                for (const bool touched : {false, true}) {
                    CAPTURE(volatility, type, asset, touched);
                    const BinaryBarrierTerms terms{
                        .option_type = type, .strike = strike, .effective_date = effective, .expiry_date = expiry, .barrier_level = spot / 2.0, .barrier_type = touched ? BarrierType::down_and_in : BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {effective}, .touch_state = touched ? BarrierTouchState::touched : BarrierTouchState::untouched};
                    const auto option = asset ? make_asset_binary_barrier_option(terms)
                                              : make_cash_binary_barrier_option(terms, payout);
                    REQUIRE(option);
                    const auto price = engine.price(*option, context);
                    const auto result = engine.price_with_greeks(*option, context, {Greek::delta});
                    REQUIRE(price);
                    REQUIRE(result);
                    CHECK(*price == *reference);
                    CHECK(result->price() == *reference);
                }
            }
        }
    }
}

TEST_CASE("Touch factories require only payoff-relevant terms")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto observation_dates = std::vector<kiyosi::Date>{effective_date + std::chrono::days{30}, expiry_date};
    const auto cash = kiyosi::make_cash_one_touch_up(
        effective_date, expiry_date, 130.0, 10.0, kiyosi::SettlementTiming::at_hit,
        kiyosi::ObservationMode::scheduled, observation_dates);
    const auto asset = kiyosi::make_asset_no_touch_down(effective_date, expiry_date, 70.0);
    REQUIRE(cash);
    REQUIRE(asset);
    CHECK(cash->is_one_touch());
    CHECK(cash->is_up());
    CHECK(cash->payoff_type() == kiyosi::PayoffType::cash);
    CHECK(cash->settlement_timing() == kiyosi::SettlementTiming::at_hit);
    CHECK(cash->observation_mode() == kiyosi::ObservationMode::scheduled);
    CHECK(cash->observation_dates() == observation_dates);
    CHECK_FALSE(asset->is_one_touch());
    CHECK_FALSE(asset->is_up());
    CHECK(asset->payoff_type() == kiyosi::PayoffType::asset);
    CHECK(asset->settlement_timing() == kiyosi::SettlementTiming::at_expiry);
}

TEST_CASE("Binary and touch contracts distinguish past settlement from future payoff")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, valuation);
    const kiyosi::AnalyticBinaryBarrierEngine engine;
    auto terms = kiyosi::BinaryBarrierTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 90.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_out};
    const auto missing = engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context);
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == kiyosi::ErrorCategory::invalid_parameter);
    terms.touch_state = kiyosi::BarrierTouchState::untouched;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) > 0.0);
    terms.touch_state = kiyosi::BarrierTouchState::touched;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) == 0.0);
    terms.barrier_type = kiyosi::BarrierType::up_and_in;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) > 0.0);

    const auto at_hit = *kiyosi::make_cash_one_touch_up(
        effective, expiry, 120.0, 10.0, kiyosi::SettlementTiming::at_hit,
        kiyosi::ObservationMode::continuous, {}, kiyosi::BarrierTouchState::touched);
    const auto at_expiry = *kiyosi::make_cash_one_touch_up(
        effective, expiry, 120.0, 10.0, kiyosi::SettlementTiming::at_expiry,
        kiyosi::ObservationMode::continuous, {}, kiyosi::BarrierTouchState::touched);
    CHECK(*engine.price(at_hit, context) == 0.0);
    CHECK_THAT(*engine.price(at_expiry, context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.04 * 184.0 / 365.0), 1e-12));
    const auto touching_now = *kiyosi::make_cash_one_touch_up(
        valuation, expiry, 90.0, 10.0, kiyosi::SettlementTiming::at_hit);
    CHECK(*engine.price(touching_now, context) == 10.0);
}

TEST_CASE("Scheduled binary and touch contracts stop monitoring after their final observation")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, day(2025, 1, 3));
    const kiyosi::AnalyticBinaryBarrierEngine engine;
    auto terms = kiyosi::BinaryBarrierTerms{.option_type = kiyosi::OptionType::call,
                                            .strike = 100.0,
                                            .effective_date = effective,
                                            .expiry_date = expiry,
                                            .barrier_level = 120.0,
                                            .barrier_type = kiyosi::BarrierType::up_and_in,
                                            .observation_mode = kiyosi::ObservationMode::scheduled,
                                            .observation_dates = {day(2025, 1, 2)},
                                            .touch_state = kiyosi::BarrierTouchState::untouched};
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) == 0.0);
    terms.barrier_type = kiyosi::BarrierType::up_and_out;
    CHECK(*engine.price(*kiyosi::make_cash_binary_barrier_option(terms, 10.0), context) > 0.0);
    const auto one_touch = *kiyosi::make_cash_one_touch_up(
        effective, expiry, 120.0, 10.0, kiyosi::SettlementTiming::at_expiry,
        kiyosi::ObservationMode::scheduled, {day(2025, 1, 2)}, kiyosi::BarrierTouchState::untouched);
    const auto no_touch = *kiyosi::make_cash_no_touch_up(
        effective, expiry, 120.0, 10.0,
        kiyosi::ObservationMode::scheduled, {day(2025, 1, 2)}, kiyosi::BarrierTouchState::untouched);
    CHECK(*engine.price(one_touch, context) == 0.0);
    CHECK_THAT(*engine.price(no_touch, context),
               Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05 * 363.0 / 365.0), 1e-12));
}
