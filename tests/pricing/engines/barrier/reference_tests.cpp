#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <memory>
#include <map>
#include <string>
#include <utility>
#include <vector>

#include "support/reference_harness.hpp"

using kiyosi::test::barrier_kinds;
using kiyosi::test::check_price;
using kiyosi::test::fixture_date;
using kiyosi::test::fixture_number;
using kiyosi::test::measures;

TEST_CASE("Finite-difference barrier spot Greeks reuse one solve", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto queries = std::make_shared<int>(0);
    const auto calendar = *make_trading_calendar([queries](Date) { ++*queries; return true; }, 365);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, calendar);
    const auto option = *make_barrier_option(
        {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}});
    const FiniteDifferenceBarrierEngine engine{800, 400};
    const auto price = engine.price(option, context);
    REQUIRE(price);
    const int price_queries = std::exchange(*queries, 0);
    REQUIRE(price_queries > 0);
    const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
    REQUIRE(result);
    CHECK(*queries == price_queries);
    CHECK(result->price() == *price);
    const auto vanilla = *make_european_option(OptionType::call, 100.0, start, end);
    const auto expected = AnalyticVanillaEngine{}.price_with_greeks(vanilla, context, {Greek::delta, Greek::gamma});
    REQUIRE(expected);
    REQUIRE(result->has(Greek::delta));
    REQUIRE(result->has(Greek::gamma));
    CHECK_THAT(*result->require(Greek::delta), Catch::Matchers::WithinAbs(*expected->require(Greek::delta), 0.001));
    CHECK_THAT(*result->require(Greek::gamma), Catch::Matchers::WithinAbs(*expected->require(Greek::gamma), 0.0001));
    CHECK_FALSE(result->has(Greek::vega));
}

TEST_CASE("Finite-difference barrier native Greeks respect monitored boundaries", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto parameters = *make_bsm_parameters(0.04, 0.01, 0.2);
    const FiniteDifferenceBarrierEngine engine{800, 400};
    for (const auto kind : {BarrierType::down_and_in, BarrierType::down_and_out, BarrierType::up_and_in, BarrierType::up_and_out}) {
        const bool up = kind == BarrierType::up_and_in || kind == BarrierType::up_and_out;
        const double barrier = up ? 120.0 : 80.0;
        const auto option = *make_barrier_option(
            {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = barrier, .barrier_type = kind});
        const auto context = *make_pricing_context(parameters, 100.0, start);
        const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        const auto expected = AnalyticBarrierEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(actual);
        REQUIRE(expected);
        REQUIRE(actual->has(Greek::delta));
        REQUIRE(actual->has(Greek::gamma));
        CAPTURE(kind);
        CHECK_THAT(*actual->require(Greek::delta), Catch::Matchers::WithinAbs(*expected->require(Greek::delta), 0.003));
        CHECK_THAT(*actual->require(Greek::gamma), Catch::Matchers::WithinAbs(*expected->require(Greek::gamma), 0.0005));
        for (const double spot : {barrier, barrier + (up ? -0.001 : 0.001)}) {
            const auto boundary = *make_pricing_context(parameters, spot, start);
            const auto result = engine.price_with_greeks(option, boundary, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(result->price() == *engine.price(option, boundary));
            CHECK_FALSE(result->has(Greek::delta));
            CHECK_FALSE(result->has(Greek::gamma));
        }
    }
}

TEST_CASE("Finite-difference barrier rebates preserve payment timing and discounting", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const FiniteDifferenceBarrierEngine engine{{.asset_step_count = 800, .time_step_count = 400, .asset_upper_boundary = 400.0}};
    for (const double rate : {-0.04, 0.04}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, 0.01, 0.2), 100.0, start);
        for (const auto kind : {BarrierType::down_and_out, BarrierType::up_and_out}) {
            for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
                CAPTURE(rate, kind, timing);
                const auto option = *make_barrier_option(
                    {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = kind == BarrierType::up_and_out ? 120.0 : 80.0, .barrier_type = kind, .rebate = 5.0, .rebate_timing = timing});
                const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                const auto expected = AnalyticBarrierEngine{}.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                REQUIRE(actual);
                REQUIRE(expected);
                CHECK(actual->price() == *engine.price(option, context));
                CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(expected->price(), 0.01));
                CHECK_THAT(*actual->require(Greek::delta), Catch::Matchers::WithinAbs(*expected->require(Greek::delta), 0.003));
                CHECK_THAT(*actual->require(Greek::gamma), Catch::Matchers::WithinAbs(*expected->require(Greek::gamma), 0.0005));
            }
        }
    }
}

TEST_CASE("Finite-difference knock-in prices preserve small positive values", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = Date{std::chrono::year{2025} / 1 / 1};
    const auto end = Date{std::chrono::year{2026} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const bool call = type == OptionType::call;
        const auto option = *make_barrier_option(
            {.option_type = type, .strike = call ? 150.0 : 50.0, .effective_date = start, .expiry_date = end, .barrier_level = call ? 70.0 : 130.0, .barrier_type = call ? BarrierType::down_and_in : BarrierType::up_and_in});
        const auto expected = AnalyticBarrierEngine{}.price(option, context);
        REQUIRE(expected);
        for (const int steps : {200, 800}) {
            CAPTURE(type, steps);
            const FiniteDifferenceBarrierEngine engine{{steps, 200}};
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price > 0.0);
            CHECK_THAT(*price, Catch::Matchers::WithinAbs(*expected, 5e-7));
            const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
            REQUIRE(joint);
            CHECK(joint->price() == *price);
        }
        const auto coarse = FiniteDifferenceBarrierEngine{{3, 200}}.price(option, context);
        REQUIRE(coarse);
        CHECK(*coarse >= 0.0);
    }
}

TEST_CASE("Finite-difference barriers preserve long-expiry volatility tails")
{
    const auto start = kiyosi::Date{std::chrono::year{2025} / 1 / 1};
    const auto end = start + std::chrono::days{1825};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.0, 0.8), 100.0, start, kiyosi::all_days_calendar());
    for (const auto mode : {kiyosi::ObservationMode::continuous, kiyosi::ObservationMode::scheduled}) {
        const auto option = *kiyosi::make_barrier_option(
            {.option_type = kiyosi::OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 0.01, .barrier_type = kiyosi::BarrierType::down_and_out, .observation_mode = mode, .observation_dates = mode == kiyosi::ObservationMode::scheduled ? std::vector<kiyosi::Date>{end} : std::vector<kiyosi::Date>{}});
        const auto expected = kiyosi::AnalyticBarrierEngine{}.price(option, context);
        REQUIRE(expected);
        for (const auto settings : {kiyosi::FiniteDifferenceSettings{}, kiyosi::FiniteDifferenceSettings{800, 1000}}) {
            CAPTURE(mode, settings.asset_step_count, settings.time_step_count);
            const auto price = kiyosi::FiniteDifferenceBarrierEngine{settings}.price(option, context);
            REQUIRE(price);
            CHECK_THAT(*price, Catch::Matchers::WithinAbs(*expected, 0.02));
        }
    }
}

TEST_CASE("Finite-difference automatic domains include distant barriers")
{
    const auto start = kiyosi::Date{std::chrono::year{2025} / 1 / 1};
    const auto end = start + std::chrono::days{365};
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.02, 0.2), 100.0, start);
    const auto option = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                      .strike = 100.0,
                                                      .effective_date = start,
                                                      .expiry_date = end,
                                                      .barrier_level = 500.0,
                                                      .barrier_type = kiyosi::BarrierType::up_and_out});
    const kiyosi::FiniteDifferenceBarrierEngine finite{800, 1000};
    const auto price = finite.price(option, context);
    REQUIRE(price);
    CHECK_THAT(*price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticBarrierEngine{}.price(option, context), 0.03));
    const auto default_price = kiyosi::FiniteDifferenceBarrierEngine{}.price(option, context);
    REQUIRE(default_price);
    CHECK_THAT(*default_price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticBarrierEngine{}.price(option, context), 0.05));
    const auto joint = finite.price_with_greeks(option, context, {kiyosi::Greek::gamma});
    REQUIRE(joint);
    CHECK(joint->price() == *price);
    const auto clipped = kiyosi::FiniteDifferenceBarrierEngine{{800, 1000, kiyosi::FiniteDifferenceScheme::crank_nicolson, 400.0}}.price(option, context);
    REQUIRE_FALSE(clipped);
    CHECK(clipped.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("QuantLib continuous barrier portfolios validate prices and numerical Greeks")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    std::map<std::string, int> generated, wrappers;
    for (const auto& fixture : cases) {
        const auto& inputs = fixture.inputs;
        if (fixture.instrument != "BarrierOption") continue;
        INFO("case=" << fixture.case_id);
        REQUIRE(inputs.at("owner") == "QuantLib");
        REQUIRE(fixture.provenance.reference_kind == "analytic");
        REQUIRE(fixture.provenance.source_symbol == "QuantLib.AnalyticBarrierEngine+QuantLib.DiscountingBondEngine+QuantLib.AnalyticEuropeanEngine");
        REQUIRE(inputs.at("monitoring") == "continuous");
        const auto number = [&](const std::string& key) { return fixture_number(fixture, key); };
        const auto date = [&](const std::string& key) { return fixture_date(fixture, key); };
        REQUIRE(barrier_kinds.contains(inputs.at("BarrierType")));
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        REQUIRE((inputs.at("settlement") == "at_hit" || inputs.at("settlement") == "at_expiry"));
        const auto option = kiyosi::make_barrier_option({.option_type = inputs.at("option") == "call" ? kiyosi::OptionType::call : kiyosi::OptionType::put,
                                                         .strike = number("strike"),
                                                         .effective_date = date("effective_date"),
                                                         .expiry_date = date("expiry_date"),
                                                         .barrier_level = number("barrier"),
                                                         .barrier_type = barrier_kinds.at(inputs.at("BarrierType")),
                                                         .rebate = number("rebate"),
                                                         .rebate_timing = inputs.at("settlement") == "at_hit" ? kiyosi::RebateTiming::at_hit
                                                                                                              : kiyosi::RebateTiming::at_expiry,
                                                         .touch_state = kiyosi::BarrierTouchState::untouched});
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(
            *parameters, number("spot"), date("valuation"), kiyosi::weekdays_calendar());
        REQUIRE(context.has_value());
        const auto check = [&](const auto& engine) {
            const auto native = engine.price(*option, *context);
            check_price(fixture, native);
            ++generated[fixture.engine];
            REQUIRE((inputs.at("wrapper") == "true" || inputs.at("wrapper") == "false"));
            const bool boundary = (date("expiry_date") - date("valuation")).count() <= 2;
            if (boundary) REQUIRE(inputs.at("wrapper") == "false");
            for (const auto& [name, value] : fixture.outputs)
                REQUIRE((name == "price" || measures.contains(name)));
            if (inputs.at("wrapper") == "false") return;
            ++wrappers[fixture.engine];
            const kiyosi::NumericalShiftSettings shifts{number("spot_shift"), number("volatility_shift"),
                                                        number("rate_shift"), static_cast<int>(number("time_shift_days"))};
            // Three nested spot shifts are used by speed; keep every stencil in the same hit state.
            REQUIRE(std::abs(number("spot") - number("barrier")) > 3 * shifts.spot_shift);
            const auto numerical = kiyosi::calculate_numerical_greeks(engine, *option, *context, shifts);
            kiyosi::test::check_numerical_result(fixture, numerical);
        };
        if (fixture.engine == "AnalyticBarrierEngine") check(kiyosi::AnalyticBarrierEngine{});
        else {
            REQUIRE(fixture.engine == "FiniteDifferenceBarrierEngine");
            REQUIRE(inputs.at("scheme") == "crank_nicolson");
            check(kiyosi::FiniteDifferenceBarrierEngine{
                {static_cast<int>(number("asset_step_count")), static_cast<int>(number("time_step_count")),
                 kiyosi::FiniteDifferenceScheme::crank_nicolson, number("asset_upper_boundary")}});
        }
    }
    REQUIRE(generated.size() == 2);
    CHECK(generated["AnalyticBarrierEngine"] == 60);
    CHECK(generated["FiniteDifferenceBarrierEngine"] == 60);
    CHECK(wrappers["AnalyticBarrierEngine"] == 48);
    CHECK(wrappers["FiniteDifferenceBarrierEngine"] == 12);
}
