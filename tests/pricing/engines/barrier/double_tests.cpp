#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <future>
#include <numbers>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"
#include "support/numerical.hpp"

namespace {
using namespace kiyosi;
constexpr auto start = test::day(2025, 1, 1);
constexpr auto expiry = test::day(2026, 1, 1);

DoubleBarrierOptionTerms terms(OptionType direction = OptionType::call, DoubleBarrierType kind = DoubleBarrierType::knock_out)
{
    return {.option_type = direction, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .lower_barrier = 80.0, .upper_barrier = 120.0, .barrier_type = kind};
}

PricingContext market(double spot = 100.0, Timestamp time = start_of_day(start), double volatility = 0.2,
                      double dividend = 0.01)
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar and is returned by value.
    return *make_pricing_context(*make_bsm_parameters(0.04, dividend, volatility), spot, time, all_days_calendar());
}

double normal(double value) { return 0.5 * std::erfc(-value / std::numbers::sqrt2); }

// Integrate the terminal payoff over the surviving lognormal interval, independently of the PDE.
double terminal_reference(double spot, OptionType direction, double lower, double upper, double time, double rebate)
{
    const double deviation = 0.2 * std::sqrt(time);
    const auto moments = [&](double left, double right) {
        const auto d1 = [&](double level) { return (std::log(spot / level) + 0.05 * time) / deviation; };
        return std::array{spot * std::exp(-0.01 * time) * (normal(d1(left)) - normal(d1(right))),
                          std::exp(-0.04 * time) * (normal(d1(left) - deviation) - normal(d1(right) - deviation))};
    };
    const double left = direction == OptionType::call ? std::max(lower, 100.0) : lower;
    const double right = direction == OptionType::call ? upper : std::min(upper, 100.0);
    const auto payoff = moments(left, right);
    const auto survival = moments(lower, upper);
    const double sign = direction == OptionType::call ? 1.0 : -1.0;
    return sign * (payoff[0] - 100.0 * payoff[1]) + rebate * (std::exp(-0.04 * time) - survival[1]);
}

double two_observation_reference(OptionType direction, RebateTiming timing, int count)
{
    constexpr double first = 180.0 / 365.0;
    const double deviation = 0.2 * std::sqrt(first);
    const double lower = (std::log(80.125 / 100.25) - 0.01 * first) / deviation;
    const double upper = (std::log(120.125 / 100.25) - 0.01 * first) / deviation;
    const double width = (upper - lower) / count;
    const auto integrand = [&](double z) {
        const double spot = 100.25 * std::exp(0.01 * first + deviation * z);
        return terminal_reference(spot, direction, 80.125, 120.125, 1.0 - first, 2.0) *
               std::exp(-0.5 * z * z) / std::sqrt(2.0 * std::numbers::pi);
    };
    double total = integrand(lower) + integrand(upper);
    for (int index = 1; index < count; ++index)
        total += (index % 2 == 0 ? 2.0 : 4.0) * integrand(lower + width * index);
    const double touched = normal(lower) + normal(-upper);
    return std::exp(-0.04 * first) * width * total / 3.0 +
           2.0 * std::exp(-0.04 * (timing == RebateTiming::at_hit ? first : 1.0)) * touched;
}
} // namespace

TEST_CASE("Double barrier continuous prices match independent references", "[double-barrier]")
{
    // QuantLib 1.43 Ikeda-Kunitomo series 5, 10 and 20 agree for this zero-rebate contract.
    for (const auto direction : {OptionType::call, OptionType::put}) {
        const double reference = direction == OptionType::call ? 1.0838814592583166 : 1.5760435147766145;
        const auto option = *make_double_barrier_option(terms(direction));
        std::array<double, 2> errors{};
        for (std::size_t index = 0; index < errors.size(); ++index) {
            const int count = index == 0 ? 80 : 320;
            const auto engine = *make_finite_difference_double_barrier_engine({count, count});
            const auto actual = engine.price(option, market());
            REQUIRE(actual);
            errors[index] = std::abs(*actual - reference);
        }
        CHECK(errors[1] < errors[0]);
        CHECK(errors[1] < 0.01);
        const auto in = *make_double_barrier_option(terms(direction, DoubleBarrierType::knock_in));
        const auto engine = *make_finite_difference_double_barrier_engine({640, 800});
        const auto actual = engine.price(in, market());
        const auto vanilla = AnalyticVanillaEngine{}.price(*make_european_option(direction, 100.0, start, expiry), market());
        REQUIRE(actual);
        REQUIRE(vanilla);
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*vanilla - reference, 0.01));
    }
}

TEST_CASE("Double barrier history and settlements retain cashflow timing", "[double-barrier]")
{
    const FiniteDifferenceDoubleBarrierEngine engine;
    for (const double spot : {79.0, 80.0, 120.0, 121.0}) {
        for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
            auto input = terms();
            input.rebate = 3.0;
            input.rebate_timing = timing;
            const auto option = *make_double_barrier_option(input);
            const auto now = engine.price(option, market(spot));
            REQUIRE(now);
            CHECK_THAT(*now, Catch::Matchers::WithinAbs(timing == RebateTiming::at_hit ? 3.0 : 3.0 * std::exp(-0.04), 1e-12));
            const auto past = *option.replace(BarrierTouchState::touched);
            const auto past_context = market(100.0, start_of_day(start) + std::chrono::days{1});
            const auto paid = engine.price(past, past_context);
            REQUIRE(paid);
            CHECK_THAT(*paid, Catch::Matchers::WithinAbs(timing == RebateTiming::at_hit ? 0.0 : 3.0 * std::exp(-0.04 * 364.0 / 365.0), 1e-12));
        }
        const auto activated = *make_double_barrier_option(terms(OptionType::call, DoubleBarrierType::knock_in));
        const auto value = engine.price(activated, market(spot));
        const auto vanilla = AnalyticVanillaEngine{}.price(*make_european_option(OptionType::call, 100.0, start, expiry), market(spot));
        REQUIRE(value);
        REQUIRE(vanilla);
        CHECK(*value == *vanilla);
    }
    const auto missing = engine.price(*make_double_barrier_option(terms()), market(100.0, start_of_day(start) + std::chrono::seconds{1}));
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == ErrorCategory::invalid_parameter);
    const auto impossible = engine.price(*make_double_barrier_option(terms())->replace(BarrierTouchState::touched), market());
    REQUIRE_FALSE(impossible);
    CHECK(impossible.error().category == ErrorCategory::invalid_option);
}

TEST_CASE("Double barrier scheduled observation history ends independently of expiry", "[double-barrier]")
{
    const auto last = start + std::chrono::days{180};
    const auto time = start_of_day(last) + std::chrono::hours{12};
    const auto context = market(130.0, time);
    for (const auto kind : {DoubleBarrierType::knock_in, DoubleBarrierType::knock_out}) {
        auto input = terms(OptionType::call, kind);
        input.rebate = 3.0;
        input.observation_mode = ObservationMode::scheduled;
        input.observation_dates = {last};
        input.touch_state = BarrierTouchState::untouched;
        const auto option = *make_double_barrier_option(input);
        const auto actual = FiniteDifferenceDoubleBarrierEngine{}.price(option, context);
        REQUIRE(actual);
        const double remaining = 184.5 / 365.0;
        const auto vanilla = AnalyticVanillaEngine{}.price(*make_european_option(OptionType::call, 100.0, start, expiry), context);
        REQUIRE(vanilla);
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(kind == DoubleBarrierType::knock_in ? 3.0 * std::exp(-0.04 * remaining) : *vanilla, 1e-12));
    }
    auto future = terms();
    future.observation_mode = ObservationMode::scheduled;
    future.observation_dates = {expiry};
    const auto actual = FiniteDifferenceDoubleBarrierEngine{}.price(*make_double_barrier_option(future), market(125.0));
    REQUIRE(actual);
    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(terminal_reference(125.0, OptionType::call, 80.0, 120.0, 1.0, 0.0), 0.002));
}

TEST_CASE("Double barrier scheduled events match independent integration", "[double-barrier]")
{
    for (const auto direction : {OptionType::call, OptionType::put})
        for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
            auto input = terms(direction);
            input.lower_barrier = 80.125;
            input.upper_barrier = 120.125;
            input.rebate = 2.0;
            input.rebate_timing = timing;
            input.observation_mode = ObservationMode::scheduled;
            for (const bool two : {false, true}) {
                input.observation_dates = two ? std::vector<Date>{start + std::chrono::days{180}, expiry} : std::vector<Date>{expiry};
                const auto option = *make_double_barrier_option(input);
                const double reference = two ? two_observation_reference(direction, timing, 1024)
                                             : terminal_reference(100.25, direction, input.lower_barrier, input.upper_barrier, 1.0, 2.0);
                if (two)
                    CHECK_THAT(reference, Catch::Matchers::WithinAbs(two_observation_reference(direction, timing, 2048), 1e-9));
                const auto engine = *make_finite_difference_double_barrier_engine({800, 2000, FiniteDifferenceScheme::crank_nicolson, 400.0});
                const auto actual = engine.price(option, market(100.25));
                REQUIRE(actual);
                CHECK_THAT(*actual, Catch::Matchers::WithinAbs(reference, 0.002));
            }
        }
}

TEST_CASE("Double barrier interim observations preserve nonnegative continuation", "[double-barrier]")
{
    auto input = terms();
    input.strike = 80.0;
    input.observation_mode = ObservationMode::scheduled;
    input.observation_dates = {start + std::chrono::days{180}};
    // Independent lognormal integration with 64 and 128 subintervals agrees to 1e-14.
    constexpr std::array scenarios{
        std::array{80.05, 80.0501, 4.679426692850714e-6},
        std::array{80.24995, 80.25005, 4.9118628396575025e-6},
        std::array{80.74995, 80.75005, 5.5303622727065195e-6}};
    constexpr std::array counts{800, 1600, 3200};
    std::array<double, counts.size()> worst{};
    for (const auto& scenario : scenarios) {
        input.lower_barrier = scenario[0];
        input.upper_barrier = scenario[1];
        const double reference = scenario[2];
        const auto option = *make_double_barrier_option(input);
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler}) {
            for (std::size_t index = 0; index < counts.size(); ++index) {
                const auto engine = *make_finite_difference_double_barrier_engine({counts[index], 2000, scheme, 400.0});
                const auto actual = engine.price(option, market(100.25));
                REQUIRE(actual);
                CHECK(*actual > 0.0);
                CHECK_THAT(*actual, Catch::Matchers::WithinAbs(reference, reference * 0.02));
                worst[index] = std::max(worst[index], std::abs(*actual - reference) / reference);
            }
        }
    }
    CHECK(worst.back() < worst.front());
    CHECK(worst.back() < 0.01);

    input.lower_barrier = 80.24995;
    input.upper_barrier = 120.0;
    const auto before_observation = start_of_day(input.observation_dates.front()) - std::chrono::seconds{1};
    for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler}) {
        const auto engine = *make_finite_difference_double_barrier_engine({800, 2000, scheme, 400.0});
        const auto actual = engine.price(*make_double_barrier_option(input), market(80.0, before_observation));
        REQUIRE(actual);
        CHECK(*actual >= 0.0);
    }
}

TEST_CASE("Double barrier in out parity includes expiry rebates and Greeks", "[double-barrier]")
{
    const auto engine = *make_finite_difference_double_barrier_engine({800, 800, FiniteDifferenceScheme::crank_nicolson, 400.0});
    for (const auto mode : {ObservationMode::continuous, ObservationMode::scheduled})
        for (const auto direction : {OptionType::call, OptionType::put}) {
            auto input = terms(direction);
            input.rebate = 2.0;
            input.observation_mode = mode;
            if (mode == ObservationMode::scheduled)
                input.observation_dates = {start + std::chrono::days{180}, expiry};
            const auto out = engine.price_with_greeks(*make_double_barrier_option(input), market(), {Greek::delta, Greek::gamma});
            input.barrier_type = DoubleBarrierType::knock_in;
            const auto in = engine.price_with_greeks(*make_double_barrier_option(input), market(), {Greek::delta, Greek::gamma});
            const auto vanilla = AnalyticVanillaEngine{}.price_with_greeks(*make_european_option(direction, 100.0, start, expiry), market(), {Greek::delta, Greek::gamma});
            REQUIRE(out);
            REQUIRE(in);
            REQUIRE(vanilla);
            CHECK_THAT(out->price() + in->price(), Catch::Matchers::WithinAbs(vanilla->price() + 2.0 * std::exp(-0.04), 0.01));
            CHECK_THAT(test::greek_value(*out, Greek::delta) + test::greek_value(*in, Greek::delta), Catch::Matchers::WithinAbs(test::greek_value(*vanilla, Greek::delta), 0.003));
            CHECK_THAT(test::greek_value(*out, Greek::gamma) + test::greek_value(*in, Greek::gamma), Catch::Matchers::WithinAbs(test::greek_value(*vanilla, Greek::gamma), 0.0005));
        }
}

TEST_CASE("Double barrier analytics respect both boundaries and fixed settlements", "[double-barrier]")
{
    const auto engine = *make_finite_difference_double_barrier_engine({400, 800});
    const auto option = *make_double_barrier_option(terms());
    for (const double spot : {80.0, 120.0}) {
        const auto result = engine.price_with_greeks(option, market(spot), true);
        REQUIRE(result);
        CHECK(result->price() == 0.0);
        for (const auto greek : {Greek::delta, Greek::gamma, Greek::vega, Greek::rho, Greek::theta})
            CHECK_FALSE(result->has(greek));
    }
    const auto context = market();
    const auto native = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::vega, Greek::rho});
    const auto bumped = test::price_differences(engine, option, context);
    REQUIRE(native);
    REQUIRE(bumped);
    CHECK(native->price() == *engine.price(option, context));
    CHECK_THAT(test::greek_value(*native, Greek::delta), Catch::Matchers::WithinAbs(test::greek_value(*bumped, Greek::delta), 0.0001));
    CHECK_THAT(test::greek_value(*native, Greek::gamma), Catch::Matchers::WithinAbs(test::greek_value(*bumped, Greek::gamma), 0.0002));
    const auto recovered = implied_volatility(engine, option, context, native->price(), {.lower_bound = 0.15, .upper_bound = 0.25});
    REQUIRE(recovered);
    CHECK_THAT(*recovered, Catch::Matchers::WithinAbs(0.2, 1e-6));
    const auto known = *option.replace(BarrierTouchState::touched);
    const auto fixed = implied_volatility(engine, known, market(100.0, start_of_day(start) + std::chrono::days{1}), 0.0);
    REQUIRE_FALSE(fixed);
    CHECK(fixed.error().category == ErrorCategory::unsupported_operation);

    auto scheduled = terms();
    scheduled.observation_mode = ObservationMode::scheduled;
    scheduled.observation_dates = {start + std::chrono::days{180}, expiry};
    scheduled.touch_state = BarrierTouchState::untouched;
    const auto event = engine.price_with_greeks(*make_double_barrier_option(scheduled), market(100.0, start_of_day(start) + std::chrono::days{180}), {Greek::theta, Greek::charm, Greek::color});
    REQUIRE(event);
    CHECK_FALSE(event->has(Greek::theta));
    CHECK_FALSE(event->has(Greek::charm));
    CHECK_FALSE(event->has(Greek::color));
}

TEST_CASE("Double barrier edge cases preserve narrow bands low volatility and strike exclusions", "[double-barrier]")
{
    auto narrow = terms();
    narrow.lower_barrier = 99.5;
    narrow.upper_barrier = 100.5;
    narrow.strike = 90.0;
    const auto option = *make_double_barrier_option(narrow);
    const auto engine = *make_finite_difference_double_barrier_engine({160, 800});
    const auto almost_certain_exit = engine.price(option, market());
    REQUIRE(almost_certain_exit);
    CHECK(*almost_certain_exit < 1e-8);
    const auto stationary = engine.price(option, market(100.0, start_of_day(start), 1e-12, 0.04));
    REQUIRE(stationary);
    CHECK_THAT(*stationary, Catch::Matchers::WithinAbs(10.0 * std::exp(-0.04), 1e-6));
    for (const auto direction : {OptionType::call, OptionType::put}) {
        auto excluded = terms(direction);
        excluded.strike = direction == OptionType::call ? 140.0 : 60.0;
        const auto out = engine.price(*make_double_barrier_option(excluded), market());
        REQUIRE(out);
        CHECK(*out == 0.0);
        excluded.barrier_type = DoubleBarrierType::knock_in;
        const auto in = engine.price(*make_double_barrier_option(excluded), market());
        const auto vanilla = AnalyticVanillaEngine{}.price(*make_european_option(direction, excluded.strike, start, expiry), market());
        REQUIRE(in);
        REQUIRE(vanilla);
        CHECK(*in == *vanilla);
    }
    const auto unstable = *make_finite_difference_double_barrier_engine({400, 1, FiniteDifferenceScheme::explicit_euler});
    const auto rejected = unstable.price(*make_double_barrier_option(terms()), market());
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_parameter);
    const auto original = *make_double_barrier_option(terms());
    auto task = std::async(std::launch::async, [&] { return engine.price(original, market()); });
    const auto concurrent = engine.price(original, market());
    const auto first = task.get();
    REQUIRE(concurrent);
    REQUIRE(first);
    CHECK(*concurrent == *first);
}

TEST_CASE("Double barrier future deterministic hits preserve rebate timing", "[double-barrier]")
{
    const auto engine = *make_finite_difference_double_barrier_engine({800, 2000});
    for (const bool upper_exit : {false, true}) {
        auto input = terms();
        input.lower_barrier = upper_exit ? 80.0 : 98.0;
        input.upper_barrier = upper_exit ? 102.0 : 120.0;
        input.rebate = 3.0;
        const double hit_time = std::log(upper_exit ? 102.0 / 100.0 : 100.0 / 98.0) / 0.04;
        const auto context = market(100.0, start_of_day(start), 1e-12, upper_exit ? 0.0 : 0.08);
        for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
            input.rebate_timing = timing;
            const auto actual = engine.price(*make_double_barrier_option(input), context);
            REQUIRE(actual);
            const double expected = 3.0 * std::exp(-0.04 * (timing == RebateTiming::at_hit ? hit_time : 1.0));
            CHECK_THAT(*actual, Catch::Matchers::WithinAbs(expected, 1e-4));
        }
    }
}

TEST_CASE("Double barrier implicit and explicit schemes retain valid prices", "[double-barrier]")
{
    for (const bool explicit_scheme : {false, true}) {
        const auto engine = *make_finite_difference_double_barrier_engine(
            {explicit_scheme ? 40 : 320, 2000,
             explicit_scheme ? FiniteDifferenceScheme::explicit_euler : FiniteDifferenceScheme::implicit_euler});
        const auto actual = engine.price(*make_double_barrier_option(terms()), market());
        REQUIRE(actual);
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(1.0838814592583166, 0.01));
    }
}
