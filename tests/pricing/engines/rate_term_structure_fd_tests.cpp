#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <cmath>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include <kiyosi/pricing/engines/binary_barrier/finite_difference.hpp>

#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;

PricingContext curved_market(bool zero_dividend = false)
{
    const auto start = start_of_day(day(2025, 1, 1));
    const auto middle = start_of_day(day(2025, 7, 1));
    const auto end = start_of_day(day(2026, 1, 1));
    const auto risk = make_rate_term_structure(start, {middle, end}, {0.99, 0.94}).value();
    const auto dividend = make_rate_term_structure(start, {start_of_day(day(2025, 4, 1)), end},
                                                   zero_dividend ? std::vector{1.0, 1.0} : std::vector{0.998, 0.98})
                              .value();
    const auto context = make_pricing_context(*make_bsm_parameters(risk, dividend, 0.2), 100.0, start, all_days_calendar());
    REQUIRE(context);
    return PricingContext{*context};
}

TEST_CASE("Binary barrier finite differences agree with flat analytic prices", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar()).value();
    const auto engine = make_finite_difference_binary_barrier_engine({800, 800}).value();
    for (const bool up : {false, true})
        for (const bool knock_in : {false, true})
            for (const bool asset : {false, true})
                for (const auto direction : {OptionType::call, OptionType::put}) {
                    const auto type = up ? (knock_in ? BarrierType::up_and_in : BarrierType::up_and_out)
                                         : (knock_in ? BarrierType::down_and_in : BarrierType::down_and_out);
                    const BinaryBarrierTerms terms{direction, 100.0, start, end, up ? 120.0 : 80.0, type};
                    const auto option = asset ? make_asset_binary_barrier_option(terms).value() : make_cash_binary_barrier_option(terms, 10.0).value();
                    const auto actual = engine.price(option, context);
                    const auto expected = AnalyticBinaryBarrierEngine{}.price(option, context);
                    CAPTURE(up, knock_in, asset, direction);
                    REQUIRE(actual);
                    REQUIRE(expected);
                    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*expected, asset ? 0.025 : 0.0025));
                }
}

TEST_CASE("Touch finite differences preserve cash asset and settlement timing", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar()).value();
    const auto engine = make_finite_difference_binary_barrier_engine({800, 800}).value();
    for (const bool up : {false, true})
        for (const bool asset : {false, true})
            for (const auto timing : {SettlementTiming::at_hit, SettlementTiming::at_expiry}) {
                const double barrier = up ? 120.0 : 80.0;
                const auto option = asset ? (up ? make_asset_one_touch_up(start, end, barrier, timing) : make_asset_one_touch_down(start, end, barrier, timing)).value()
                                          : (up ? make_cash_one_touch_up(start, end, barrier, 10.0, timing) : make_cash_one_touch_down(start, end, barrier, 10.0, timing)).value();
                const auto actual = engine.price(option, context);
                const auto expected = AnalyticBinaryBarrierEngine{}.price(option, context);
                CAPTURE(up, asset, timing);
                REQUIRE(actual);
                REQUIRE(expected);
                CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*expected, asset ? 0.025 : 0.0025));
            }
}

TEST_CASE("Nonflat binary and touch finite differences preserve payoff parity", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto context = curved_market();
    const auto engine = make_finite_difference_binary_barrier_engine({800, 800}).value();
    for (const bool up : {false, true})
        for (const bool asset : {false, true})
            for (const bool scheduled : {false, true}) {
                const double barrier = up ? 120.0 : 80.0;
                const auto mode = scheduled ? ObservationMode::scheduled : ObservationMode::continuous;
                const auto dates = scheduled ? std::vector{middle, end} : std::vector<Date>{};
                BinaryBarrierTerms terms{OptionType::call, 100.0, start, end, barrier,
                                         up ? BarrierType::up_and_in : BarrierType::down_and_in, mode, dates};
                const auto inside = asset ? make_asset_binary_barrier_option(terms).value() : make_cash_binary_barrier_option(terms, 10.0).value();
                terms.barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out;
                const auto outside = asset ? make_asset_binary_barrier_option(terms).value() : make_cash_binary_barrier_option(terms, 10.0).value();
                const auto first = engine.price(inside, context), second = engine.price(outside, context);
                const auto vanilla = asset ? AnalyticDigitalEngine{}.price(*make_asset_or_nothing_option(OptionType::call, 100.0, start, end), context)
                                           : AnalyticDigitalEngine{}.price(*make_cash_or_nothing_option(OptionType::call, 100.0, 10.0, start, end), context);
                CAPTURE(up, asset, scheduled);
                REQUIRE(first);
                REQUIRE(second);
                REQUIRE(vanilla);
                CHECK_THAT(*first + *second, Catch::Matchers::WithinAbs(*vanilla, asset ? 0.025 : 0.0025));
                const auto touched = asset ? (up ? make_asset_one_touch_up(start, end, barrier, SettlementTiming::at_expiry, mode, dates) : make_asset_one_touch_down(start, end, barrier, SettlementTiming::at_expiry, mode, dates)).value()
                                           : (up ? make_cash_one_touch_up(start, end, barrier, 10.0, SettlementTiming::at_expiry, mode, dates) : make_cash_one_touch_down(start, end, barrier, 10.0, SettlementTiming::at_expiry, mode, dates)).value();
                const auto untouched = asset ? (up ? make_asset_no_touch_up(start, end, barrier, mode, dates) : make_asset_no_touch_down(start, end, barrier, mode, dates)).value()
                                             : (up ? make_cash_no_touch_up(start, end, barrier, 10.0, mode, dates) : make_cash_no_touch_down(start, end, barrier, 10.0, mode, dates)).value();
                const auto one = engine.price(touched, context), no = engine.price(untouched, context);
                REQUIRE(one);
                REQUIRE(no);
                CHECK_THAT(*one + *no, Catch::Matchers::WithinAbs(asset ? 98.0 : 9.4, asset ? 0.003 : 0.0003));
            }
}

TEST_CASE("Nonflat American and barrier finite differences match vanilla identities", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = curved_market(true);
    const auto european = make_european_option(OptionType::call, 100.0, start, end).value();
    const auto american = make_american_option(OptionType::call, 100.0, start, end).value();
    const auto expected = AnalyticVanillaEngine{}.price(european, context);
    REQUIRE(expected);
    const auto vanilla_engine = make_finite_difference_vanilla_engine({800, 800}).value();
    const auto actual = vanilla_engine.price(american, context);
    REQUIRE(actual);
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*expected, 0.004));

    const auto engine = make_finite_difference_barrier_engine({800, 800}).value();
    const auto in = make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_in}).value();
    const auto out = make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out}).value();
    const auto first = engine.price(in, context), second = engine.price(out, context);
    REQUIRE(first);
    REQUIRE(second);
    CHECK_THAT(*first + *second, Catch::Matchers::WithinAbs(*expected, 0.004));
}

TEST_CASE("Nonflat structured finite differences retain terminal payoff decompositions", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = curved_market();
    const auto note = make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}}).value();
    const auto digital = AnalyticDigitalEngine{}.price(*make_cash_or_nothing_option(OptionType::call, 100.0, 0.1, start, end), context);
    REQUIRE(digital);
    const auto note_price = make_finite_difference_autocallable_engine({800, 800}).value().price(note, context);
    REQUIRE(note_price);
    CHECK_THAT(*note_price, Catch::Matchers::WithinAbs(0.94 + *digital, 1e-5));
    const auto middle = day(2025, 7, 1);
    const double coupon_rate = 0.1 / *year_fraction(start, middle);
    const auto early = make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{middle, 1.0, coupon_rate}, {end, 1.0, 0.1}}}).value();
    const auto early_price = make_finite_difference_autocallable_engine({800, 800}).value().price(early, context);
    REQUIRE(early_price);
    CHECK_THAT(*early_price, Catch::Matchers::WithinAbs(1.1 * 0.99, 1e-5));
    const auto accumulator = make_accumulator({100.0, 399.0, 0.0, 1.0, 2.0, start, end}).value();
    const auto accumulator_price = make_finite_difference_accumulator_engine({800, 800, FiniteDifferenceScheme::crank_nicolson, 400.0}).value().price(accumulator, context);
    REQUIRE(accumulator_price);
    CHECK_THAT(*accumulator_price, Catch::Matchers::WithinAbs(2.0 * (98.0 - 94.0), 0.002));
}

TEST_CASE("Barrier finite differences retain the shape of nonflat rate curves", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto context = curved_market();
    const auto front_loaded = make_rate_term_structure(start_of_day(start), {start_of_day(middle), start_of_day(end)}, {0.95, 0.94}).value();
    const auto alternate = make_pricing_context(*make_bsm_parameters(front_loaded, context.model_parameters().dividend_curve(), 0.2),
                                                100.0, start, all_days_calendar())
                               .value();
    const auto vanilla = make_european_option(OptionType::call, 100.0, start, end).value();
    const auto vanilla_first = AnalyticVanillaEngine{}.price(vanilla, context);
    const auto vanilla_second = AnalyticVanillaEngine{}.price(vanilla, alternate);
    REQUIRE(vanilla_first);
    REQUIRE(vanilla_second);
    CHECK_THAT(*vanilla_first, Catch::Matchers::WithinAbs(*vanilla_second, 1e-12));
    const auto option = make_cash_one_touch_up(start, end, 120.0, 10.0).value();
    const auto engine = make_finite_difference_binary_barrier_engine({800, 800}).value();
    const auto first = engine.price(option, context), second = engine.price(option, alternate);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(std::abs(*first - *second) > 0.01);
}

TEST_CASE("Binary barrier finite differences converge toward an independent flat formula", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar()).value();
    const auto option = make_cash_one_touch_up(start, end, 120.0, 10.0, SettlementTiming::at_hit).value();
    const auto expected = AnalyticBinaryBarrierEngine{}.price(option, context);
    REQUIRE(expected);
    std::array<double, 3> errors{};
    const std::array counts{200, 400, 800};
    for (std::size_t index = 0; index < counts.size(); ++index) {
        const auto actual = make_finite_difference_binary_barrier_engine({counts[index], counts[index]}).value().price(option, context);
        REQUIRE(actual);
        errors[index] = std::abs(*actual - *expected);
    }
    CHECK(errors[1] < errors[0]);
    CHECK(errors[2] < errors[1]);
    CHECK(errors[2] < 0.001);
}

TEST_CASE("Scheduled at-hit touch uses only its monitoring horizon", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), observation = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto risk = make_rate_term_structure(start_of_day(start), {start_of_day(observation)}, {0.99}).value();
    const auto dividend = make_rate_term_structure(start_of_day(start), {start_of_day(observation)}, {0.98}).value();
    const auto context = make_pricing_context(*make_bsm_parameters(risk, dividend, 0.2), 100.0, start, all_days_calendar()).value();
    const auto engine = make_finite_difference_binary_barrier_engine({800, 800}).value();
    for (const bool up : {false, true})
        for (const bool asset : {false, true}) {
            const double barrier = up ? 120.0 : 80.0;
            const auto option = asset ? (up ? make_asset_one_touch_up(start, end, barrier, SettlementTiming::at_hit, ObservationMode::scheduled, {observation})
                                            : make_asset_one_touch_down(start, end, barrier, SettlementTiming::at_hit, ObservationMode::scheduled, {observation}))
                                            .value()
                                      : (up ? make_cash_one_touch_up(start, end, barrier, 10.0, SettlementTiming::at_hit, ObservationMode::scheduled, {observation})
                                            : make_cash_one_touch_down(start, end, barrier, 10.0, SettlementTiming::at_hit, ObservationMode::scheduled, {observation}))
                                            .value();
            const auto direction = up ? OptionType::call : OptionType::put;
            const auto expected = asset ? AnalyticDigitalEngine{}.price(*make_asset_or_nothing_option(direction, barrier, start, observation), context)
                                        : AnalyticDigitalEngine{}.price(*make_cash_or_nothing_option(direction, barrier, 10.0, start, observation), context);
            const auto actual = engine.price(option, context);
            CAPTURE(up, asset);
            REQUIRE(expected);
            REQUIRE(actual);
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*expected, asset ? 0.01 : 0.001));
        }
    const auto double_touch = make_cash_double_touch(start, end, 80.0, 120.0, 10.0, SettlementTiming::at_hit,
                                                     ObservationMode::scheduled, {observation})
                                  .value();
    const auto lower = AnalyticDigitalEngine{}.price(*make_cash_or_nothing_option(OptionType::put, 80.0, 10.0, start, observation), context);
    const auto upper = AnalyticDigitalEngine{}.price(*make_cash_or_nothing_option(OptionType::call, 120.0, 10.0, start, observation), context);
    const auto actual = make_finite_difference_double_touch_engine({800, 800}).value().price(double_touch, context);
    REQUIRE(lower);
    REQUIRE(upper);
    REQUIRE(actual);
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*lower + *upper, 0.001));
}

TEST_CASE("Binary barrier finite differences reject unstable grids and preserve paid settlements", "[rate-curves-fd]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = curved_market();
    const auto option = make_cash_one_touch_up(start, end, 120.0, 10.0).value();
    const auto unstable = make_finite_difference_binary_barrier_engine({800, 1, FiniteDifferenceScheme::explicit_euler}).value().price(option, context);
    REQUIRE_FALSE(unstable);
    CHECK(unstable.error().category == ErrorCategory::invalid_parameter);
    const auto valuation = start_of_day(day(2025, 7, 1));
    const auto short_curve = make_rate_term_structure(start_of_day(start), {valuation}, {0.99}).value();
    const auto expired_coverage = make_pricing_context(*make_bsm_parameters(short_curve, short_curve, 0.2), 100.0, valuation, all_days_calendar()).value();
    const auto paid = make_cash_one_touch_up(start, end, 120.0, 10.0, SettlementTiming::at_hit,
                                             ObservationMode::continuous, {}, BarrierTouchState::touched)
                          .value();
    const auto actual = FiniteDifferenceBinaryBarrierEngine{}.price(paid, expired_coverage);
    REQUIRE(actual);
    CHECK(*actual == 0.0);
}

} // namespace
