#include <catch2/catch_test_macros.hpp>

#include <limits>
#include <optional>
#include <type_traits>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;
} // namespace

TEST_CASE("Autocallable replace preserves all terms and permits history corrections", "[history-replace]")
{
    const auto start = day(2025, 1, 1), first = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto check = [](auto factory, const auto& terms) {
        const auto original = factory(terms);
        REQUIRE(original);
        for (const auto state : {std::optional{AutocallableBarrierState::knocked_out}, std::optional{AutocallableBarrierState::none}, std::optional<AutocallableBarrierState>{}}) {
            auto expected_terms = terms;
            expected_terms.barrier_state = state;
            const auto expected = factory(expected_terms);
            const auto replaced = original->replace(state);
            REQUIRE(expected);
            REQUIRE(replaced);
            CHECK(replaced->barrier_state() == state);
            CHECK(*replaced == *expected);
            CHECK(replaced->observations_view().data() != original->observations_view().data());
            const auto corrected = replaced->replace(terms.barrier_state);
            REQUIRE(corrected);
            CHECK(*corrected == *original);
            CHECK(replaced->barrier_state() == state);
            CHECK(*original == *factory(terms));
        }
        const auto invalid = original->replace(static_cast<AutocallableBarrierState>(255));
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
        if constexpr (std::is_same_v<std::remove_cvref_t<decltype(terms)>, BinarySnowballTerms>) {
            const auto knocked_in = original->replace(AutocallableBarrierState::knocked_in);
            REQUIRE_FALSE(knocked_in);
            CHECK(knocked_in.error().category == ErrorCategory::invalid_parameter);
        } else {
            auto expected_terms = terms;
            expected_terms.barrier_state = AutocallableBarrierState::knocked_in;
            const auto knocked_in = original->replace(AutocallableBarrierState::knocked_in);
            REQUIRE(knocked_in);
            CHECK(*knocked_in == *factory(expected_terms));
        }
        CHECK(*original == *factory(terms));
    };
    check(make_snowball_option, SnowballTerms{.maturity_coupon_rate = 0.0625, .initial_spot = 105.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 55.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::knocked_in, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.125}, {end, 110.0, 0.25}}});
    check(make_binary_snowball_option, BinarySnowballTerms{.maturity_coupon_rate = 0.0625, .barrier_state = AutocallableBarrierState::none, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.125}, {end, 110.0, 0.25}}});
    check(make_ternary_snowball_option, TernarySnowballTerms{.maturity_coupon_rate = 0.0625, .minimum_coupon_rate = 0.015625, .knock_in_level = 75.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::knocked_in, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.125}, {end, 110.0, 0.25}}});
    check(make_phoenix_option, PhoenixTerms{.coupon_rate = 0.125, .initial_spot = 105.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 55.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.0, 85.0}, {end, 110.0, 0.0, 80.0}}});
}

TEST_CASE("Barrier replace preserves payoff schedule and allows clearing touch history", "[history-replace]")
{
    const auto start = day(2025, 1, 1), first = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto check = [](auto factory) {
        const auto original = factory(BarrierTouchState::touched);
        REQUIRE(original);
        for (const auto state : {std::optional{BarrierTouchState::untouched}, std::optional{BarrierTouchState::touched}, std::optional<BarrierTouchState>{}}) {
            const auto replaced = original->replace(state);
            const auto expected = factory(state);
            REQUIRE(replaced);
            REQUIRE(expected);
            CHECK(replaced->touch_state() == state);
            CHECK(*replaced == *expected);
            CHECK(*original == *factory(BarrierTouchState::touched));
        }
        const auto invalid = original->replace(static_cast<BarrierTouchState>(255));
        const auto invalid_construction = factory(static_cast<BarrierTouchState>(255));
        REQUIRE_FALSE(invalid);
        REQUIRE_FALSE(invalid_construction);
        CHECK(invalid.error().category == ErrorCategory::invalid_option);
        CHECK(invalid.error().category == invalid_construction.error().category);
        CHECK(*original == *factory(BarrierTouchState::touched));
    };
    check([&](std::optional<BarrierTouchState> state) {
        return make_barrier_option({.option_type = OptionType::put, .strike = 103, .effective_date = start, .expiry_date = end, .barrier_level = 120, .barrier_type = BarrierType::up_and_out, .rebate = 7, .rebate_timing = RebateTiming::at_hit, .observation_mode = ObservationMode::scheduled, .observation_dates = {first, end}, .touch_state = state});
    });
    const auto binary_terms = [&](std::optional<BarrierTouchState> state) {
        return BinaryBarrierTerms{.option_type = OptionType::put, .strike = 103, .effective_date = start, .expiry_date = end, .barrier_level = 120, .barrier_type = BarrierType::up_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {first, end}, .touch_state = state};
    };
    check([&](std::optional<BarrierTouchState> state) { return make_cash_binary_barrier_option(binary_terms(state), 17); });
    check([&](std::optional<BarrierTouchState> state) { return make_asset_binary_barrier_option(binary_terms(state)); });
    check([&](std::optional<BarrierTouchState> state) { return make_cash_one_touch_up(start, end, 120, 19, SettlementTiming::at_expiry, ObservationMode::scheduled, {first, end}, state); });
    check([&](std::optional<BarrierTouchState> state) { return make_asset_no_touch_down(start, end, 75, ObservationMode::scheduled, {first, end}, state); });
}

TEST_CASE("Numeric history replace preserves terms and validates corrections", "[history-replace]")
{
    const auto start = day(2025, 1, 1), first = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto check = [](auto factory) {
        const auto original = factory(99);
        REQUIRE(original);
        for (const double history : {102.5, 0.0, 1.0}) {
            const auto replaced = original->replace(history);
            const auto expected = factory(history);
            REQUIRE(replaced);
            REQUIRE(expected);
            CHECK(*replaced == *expected);
            const auto corrected = replaced->replace(99);
            REQUIRE(corrected);
            CHECK(*corrected == *original);
            CHECK(*replaced == *expected);
            CHECK(*original == *factory(99));
        }
        for (const double invalid : {-1.0, std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity(), -std::numeric_limits<double>::infinity()}) {
            const auto rejected = original->replace(invalid);
            REQUIRE_FALSE(rejected);
            CHECK(rejected.error().category == ErrorCategory::invalid_parameter);
            CHECK(*original == *factory(99));
        }
    };
    check([&](double average) { return make_geometric_average_option(OptionType::put, 103, start, first, end, average); });
    check([&](double average) { return make_arithmetic_average_option(OptionType::put, 103, start, first, end, average); });
    check([&](double quantity) { return make_accumulator({.strike = 103, .knock_out_level = 120, .daily_quantity = 3, .acceleration_factor = 2.5, .accumulated_quantity = quantity, .effective_date = start, .expiry_date = end}); });
}

TEST_CASE("History replace defers valuation history checks until pricing", "[history-replace]")
{
    const auto option = make_barrier_option({.option_type = OptionType::call, .strike = 100, .effective_date = day(2025, 1, 1), .expiry_date = day(2026, 1, 1), .barrier_level = 120, .barrier_type = BarrierType::up_and_out, .touch_state = BarrierTouchState::untouched});
    REQUIRE(option);
    const auto unknown = option->replace(std::nullopt);
    REQUIRE(unknown);
    const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100, day(2025, 7, 1));
    REQUIRE(context);
    const AnalyticBarrierEngine engine;
    const auto missing = engine.price(*unknown, *context);
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == ErrorCategory::invalid_parameter);
    const auto untouched = unknown->replace(BarrierTouchState::untouched);
    const auto touched = option->replace(BarrierTouchState::touched);
    REQUIRE(untouched);
    REQUIRE(touched);
    const auto live = engine.price(*untouched, *context);
    const auto settled = engine.price(*touched, *context);
    REQUIRE(live);
    REQUIRE(settled);
    CHECK(*live > 0);
    CHECK(*settled == 0);
    CHECK(option->touch_state() == BarrierTouchState::untouched);
    CHECK_FALSE(unknown->touch_state());
}
