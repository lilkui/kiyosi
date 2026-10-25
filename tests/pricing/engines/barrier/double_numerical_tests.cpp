#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <cmath>
#include <future>
#include <numbers>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "pricing/engines/monte_carlo_host.hpp"
#include "support/common.hpp"

namespace {
using namespace kiyosi;
constexpr auto start = test::day(2025, 1, 1);
constexpr auto expiry = test::day(2026, 1, 1);
constexpr auto first = start + std::chrono::days{180};

DoubleBarrierOptionTerms terms(OptionType direction = OptionType::call, DoubleBarrierType kind = DoubleBarrierType::knock_out)
{
    return {.option_type = direction, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .lower_barrier = 80.0, .upper_barrier = 120.0, .barrier_type = kind};
}

PricingContext market(double spot = 100.0, Timestamp time = start_of_day(start), double rate = 0.04,
                      double dividend = 0.01, double volatility = 0.2)
{
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns the returned calendar.
    return *make_pricing_context(*make_bsm_parameters(rate, dividend, volatility), spot, time, all_days_calendar());
}

template <typename Check>
void with_engines(const Check& check)
{
    {
        INFO("AnalyticDoubleBarrierEngine");
        check(AnalyticDoubleBarrierEngine{});
    }
    {
        INFO("MonteCarloDoubleBarrierEngine");
        check(*make_monte_carlo_double_barrier_engine({.path_count = 32'768, .seed = 42, .sampling = MonteCarloSampling::sobol}));
    }
}

double normal(double value) { return 0.5 * std::erfc(-value / std::numbers::sqrt2); }

double terminal_reference(OptionType direction, double rebate)
{
    const auto moment = [&](double low, double high) {
        const auto d1 = [](double level) { return (std::log(100.0 / level) + 0.05) / 0.2; };
        return std::array{100.0 * std::exp(-0.01) * (normal(d1(low)) - normal(d1(high))),
                          std::exp(-0.04) * (normal(d1(low) - 0.2) - normal(d1(high) - 0.2))};
    };
    const auto payoff = direction == OptionType::call ? moment(100.0, 120.0) : moment(80.0, 100.0);
    const double sign = direction == OptionType::call ? 1.0 : -1.0;
    return sign * (payoff[0] - 100.0 * payoff[1]) + rebate * (std::exp(-0.04) - moment(80.0, 120.0)[1]);
}

// Independently integrate the driftless killed-Brownian Fourier survival density and exit time.
std::array<double, 3> rebate_reference(double rate, int count)
{
    const double width = std::log(125.0 / 80.0);
    double survival = 0.0, integral = 0.0;
    for (int index = 0; index < count; ++index) {
        const double frequency = 2.0 * index + 1.0;
        const double coefficient = 4.0 * (index % 2 == 0 ? 1.0 : -1.0) / (frequency * std::numbers::pi);
        const double decay = 0.5 * 0.2 * 0.2 * std::pow(frequency * std::numbers::pi / width, 2);
        survival += coefficient * std::exp(-decay);
        integral += coefficient * -std::expm1(-(rate + decay)) / (rate + decay);
    }
    const double discounted = std::exp(-rate) * survival;
    return {3.0 * (std::exp(-rate) - discounted), 3.0 * (1.0 - discounted - rate * integral), 3.0 * discounted};
}
} // namespace

TEST_CASE("Double barrier analytic prices and Greeks match independent QuantLib values", "[double-barrier-numerical]")
{
    const AnalyticDoubleBarrierEngine engine;
    for (const auto direction : {OptionType::call, OptionType::put}) {
        const bool call = direction == OptionType::call;
        // QuantLib Ikeda-Kunitomo 20-term values; spot derivatives use independent 0.01 shifts.
        const double reference = call ? 1.0838814592583166 : 1.5760435147766145;
        const double delta = call ? -0.009437716341942348 : -0.031725540191196444;
        const double gamma = call ? -0.006614552070516311 : -0.008683048910995694;
        const auto out = engine.price_with_greeks(*make_double_barrier_option(terms(direction)), market(), {Greek::delta, Greek::gamma});
        REQUIRE(out);
        CHECK(out->price() == Catch::Approx(reference).margin(1e-9));
        CHECK(*out->require(Greek::delta) == Catch::Approx(delta).margin(1e-7));
        CHECK(*out->require(Greek::gamma) == Catch::Approx(gamma).margin(1e-6));
        CHECK_FALSE(out->has(Greek::vega));
        const auto in = engine.price_with_greeks(*make_double_barrier_option(terms(direction, DoubleBarrierType::knock_in)), market(), {Greek::delta, Greek::gamma});
        const auto vanilla = AnalyticVanillaEngine{}.price_with_greeks(*make_european_option(direction, 100.0, start, expiry), market(), {Greek::delta, Greek::gamma});
        REQUIRE(in);
        REQUIRE(vanilla);
        CHECK(in->price() + out->price() == Catch::Approx(vanilla->price()).margin(1e-9));
        CHECK(*in->require(Greek::delta) + *out->require(Greek::delta) == Catch::Approx(*vanilla->require(Greek::delta)).margin(1e-6));
        CHECK(*in->require(Greek::gamma) + *out->require(Greek::gamma) == Catch::Approx(*vanilla->require(Greek::gamma)).margin(1e-6));
    }
}

TEST_CASE("Double barrier MC prices and likelihood Greeks match references across seeds", "[double-barrier-numerical]")
{
    for (const auto direction : {OptionType::call, OptionType::put}) {
        const bool call = direction == OptionType::call;
        for (const auto kind : {DoubleBarrierType::knock_in, DoubleBarrierType::knock_out}) {
            const auto vanilla = AnalyticVanillaEngine{}.price_with_greeks(*make_european_option(direction, 100.0, start, expiry), market(), {Greek::delta, Greek::gamma}).value();
            double price = call ? 1.0838814592583166 : 1.5760435147766145;
            double delta = call ? -0.009437716341942348 : -0.031725540191196444;
            double gamma = call ? -0.006614552070516311 : -0.008683048910995694;
            if (kind == DoubleBarrierType::knock_in) {
                price = vanilla.price() - price;
                delta = *vanilla.require(Greek::delta) - delta;
                gamma = *vanilla.require(Greek::gamma) - gamma;
            }
            for (const auto seed : {17U, 42U, 103U}) {
                const auto engine = make_monte_carlo_double_barrier_engine({.path_count = 131'072, .seed = seed, .antithetic = true}).value();
                const auto actual = engine.price_with_greeks(*make_double_barrier_option(terms(direction, kind)), market(), true);
                REQUIRE(actual);
                CHECK(actual->price() == Catch::Approx(price).margin(0.12));
                CHECK(*actual->require(Greek::delta) == Catch::Approx(delta).margin(0.015));
                CHECK(*actual->require(Greek::gamma) == Catch::Approx(gamma).margin(0.003));
                for (const auto greek : {Greek::speed, Greek::theta, Greek::charm, Greek::color, Greek::vega, Greek::vanna, Greek::zomma, Greek::rho})
                    CHECK_FALSE(actual->has(greek));
            }
        }
    }
}

TEST_CASE("Double barrier image and spectral series match references across the switch", "[double-barrier-numerical]")
{
    // QuantLib Ikeda-Kunitomo 20-term values straddling sigma*sqrt(T)/log(U/L) = 0.5.
    constexpr std::array volatilities{0.20273255202675663, 0.20273255608140772, 0.3040988310811233};
    constexpr std::array prices{1.045594059652574, 1.0455940036319245, 0.21271328442609105};
    constexpr std::array deltas{-0.009496602827940848, -0.009496602869507598, -0.002243229044052164};
    constexpr std::array gammas{-0.006356529009821088, -0.006356530679596517, -0.0012635228285873268};
    for (std::size_t index = 0; index < prices.size(); ++index) {
        const auto context = market(100.0, start_of_day(start), 0.04, 0.01, volatilities[index]);
        with_engines([&](const auto& engine) {
            const auto result = engine.price_with_greeks(*make_double_barrier_option(terms()), context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            constexpr bool analytic = std::same_as<std::remove_cvref_t<decltype(engine)>, AnalyticDoubleBarrierEngine>;
            CHECK(result->price() == Catch::Approx(prices[index]).margin(analytic ? 1e-9 : 0.003));
            CHECK(*result->require(Greek::delta) == Catch::Approx(deltas[index]).margin(analytic ? 1e-7 : 0.003));
            CHECK(*result->require(Greek::gamma) == Catch::Approx(gammas[index]).margin(analytic ? 1e-6 : 0.003));
        });
    }
}

TEST_CASE("Double barrier rebates match independent exit series with negative rates", "[double-barrier-numerical]")
{
    for (const double rate : {-0.02, 0.04}) {
        const auto references = rebate_reference(rate, 4097);
        const auto refined = rebate_reference(rate, 8193);
        for (std::size_t index = 0; index < references.size(); ++index)
            CHECK(references[index] == Catch::Approx(refined[index]).margin(1e-11));
        const auto context = market(100.0, start_of_day(start), rate, rate - 0.02);
        with_engines([&](const auto& engine) {
            for (int index = 0; index < 3; ++index) {
                auto input = terms(OptionType::call, index == 2 ? DoubleBarrierType::knock_in : DoubleBarrierType::knock_out);
                input.upper_barrier = 125.0;
                input.rebate_timing = index == 1 ? RebateTiming::at_hit : RebateTiming::at_expiry;
                const auto base = engine.price(*make_double_barrier_option(input), context);
                input.rebate = 3.0;
                const auto actual = engine.price(*make_double_barrier_option(input), context);
                REQUIRE(base);
                REQUIRE(actual);
                constexpr double tolerance = std::same_as<std::remove_cvref_t<decltype(engine)>, AnalyticDoubleBarrierEngine> ? 1e-9 : 0.004;
                CHECK(*actual - *base == Catch::Approx(references[static_cast<std::size_t>(index)]).margin(tolerance));
            }
        });
    }
}

TEST_CASE("Double barrier scheduled MC uses actual dates and analytic rejects unresolved schedules", "[double-barrier-numerical]")
{
    const auto engine = make_monte_carlo_double_barrier_engine({.path_count = 262'144, .seed = 42, .sampling = MonteCarloSampling::sobol}).value();
    for (const auto direction : {OptionType::call, OptionType::put}) {
        auto input = terms(direction);
        input.rebate = 2.0;
        input.observation_mode = ObservationMode::scheduled;
        input.observation_dates = {expiry};
        const auto option = make_double_barrier_option(input).value();
        const auto actual = engine.price(option, market());
        REQUIRE(actual);
        CHECK(*actual == Catch::Approx(terminal_reference(direction, 2.0)).margin(0.002));
        const auto unsupported = AnalyticDoubleBarrierEngine{}.price(option, market());
        REQUIRE_FALSE(unsupported);
        CHECK(unsupported.error().category == ErrorCategory::unsupported_operation);
        for (const bool final : {false, true}) {
            input.observation_dates = final ? std::vector{first, expiry} : std::vector{first};
            const auto scheduled = make_double_barrier_option(input).value();
            const auto estimate = engine.price_with_greeks(scheduled, market(), {Greek::delta, Greek::gamma});
            const auto reference = make_finite_difference_double_barrier_engine({1200, 1200})->price_with_greeks(scheduled, market(), {Greek::delta, Greek::gamma});
            REQUIRE(estimate);
            REQUIRE(reference);
            CHECK(estimate->price() == Catch::Approx(reference->price()).margin(0.03));
            CHECK(*estimate->require(Greek::delta) == Catch::Approx(*reference->require(Greek::delta)).margin(0.025));
            CHECK(*estimate->require(Greek::gamma) == Catch::Approx(*reference->require(Greek::gamma)).margin(0.01));
        }
    }
}

TEST_CASE("Double barrier engines preserve current and prior touches history expiry and finished monitoring", "[double-barrier-numerical]")
{
    const auto later = market(100.0, start_of_day(start) + std::chrono::days{1});
    with_engines([&](const auto& engine) {
        for (const double spot : {79.0, 80.0, 120.0, 121.0}) {
            for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
                auto input = terms();
                input.rebate = 3.0;
                input.rebate_timing = timing;
                const auto option = make_double_barrier_option(input).value();
                const auto actual = engine.price(option, market(spot));
                REQUIRE(actual);
                CHECK(*actual == Catch::Approx(timing == RebateTiming::at_hit ? 3.0 : 3.0 * std::exp(-0.04)).margin(1e-12));
                const auto paid = engine.price(*option.replace(BarrierTouchState::touched), later);
                REQUIRE(paid);
                CHECK(*paid == Catch::Approx(timing == RebateTiming::at_hit ? 0.0 : 3.0 * std::exp(-0.04 * 364.0 / 365.0)).margin(1e-12));
                if (spot == 80.0 || spot == 120.0) {
                    const auto boundary = engine.price_with_greeks(option, market(spot), true);
                    REQUIRE(boundary);
                    CHECK_FALSE(boundary->has(Greek::delta));
                    CHECK_FALSE(boundary->has(Greek::gamma));
                }
            }
        }
        const auto missing = engine.price(*make_double_barrier_option(terms()), later);
        REQUIRE_FALSE(missing);
        CHECK(missing.error().category == ErrorCategory::invalid_parameter);
        const auto impossible = engine.price(*make_double_barrier_option(terms())->replace(BarrierTouchState::touched), market());
        REQUIRE_FALSE(impossible);
        CHECK(impossible.error().category == ErrorCategory::invalid_option);
        auto input = terms(OptionType::call, DoubleBarrierType::knock_in);
        input.rebate = 2.0;
        input.touch_state = BarrierTouchState::untouched;
        const auto expired = engine.price_with_greeks(*make_double_barrier_option(input), market(100.0, start_of_day(expiry)), true);
        INFO((expired ? "expiry priced" : expired.error().message));
        REQUIRE(expired);
        CHECK(expired->price() == 2.0);
        CHECK_FALSE(expired->has(Greek::delta));
        CHECK_FALSE(expired->has(Greek::gamma));
        for (const auto kind : {DoubleBarrierType::knock_in, DoubleBarrierType::knock_out}) {
            input.barrier_type = kind;
            input.rebate = 3.0;
            input.observation_mode = ObservationMode::scheduled;
            input.observation_dates = {first};
            const auto context = market(130.0, start_of_day(first) + std::chrono::days{1});
            const auto completed = engine.price(*make_double_barrier_option(input), context);
            const auto vanilla = AnalyticVanillaEngine{}.price(*make_european_option(OptionType::call, 100.0, start, expiry), context);
            REQUIRE(completed);
            REQUIRE(vanilla);
            CHECK(*completed == Catch::Approx(kind == DoubleBarrierType::knock_in ? 3.0 * std::exp(-0.04 * 184.0 / 365.0) : *vanilla).margin(1e-12));
        }
    });
}

TEST_CASE("Double barrier engines handle narrow wide bands excluded strikes and tiny volatility hits", "[double-barrier-numerical]")
{
    with_engines([&](const auto& engine) {
        auto input = terms();
        input.lower_barrier = 99.99;
        input.upper_barrier = 100.01;
        const auto narrow = engine.price(*make_double_barrier_option(input), market());
        REQUIRE(narrow);
        CHECK(*narrow == Catch::Approx(0.0).margin(1e-12));
        input.lower_barrier = 0.01;
        input.upper_barrier = 10'000.0;
        const auto wide = engine.price(*make_double_barrier_option(input), market());
        const auto vanilla = AnalyticVanillaEngine{}.price(*make_european_option(OptionType::call, 100.0, start, expiry), market());
        REQUIRE(wide);
        REQUIRE(vanilla);
        constexpr double tolerance = std::same_as<std::remove_cvref_t<decltype(engine)>, AnalyticDoubleBarrierEngine> ? 1e-9 : 0.025;
        CHECK(*wide == Catch::Approx(*vanilla).margin(tolerance));
        for (const auto direction : {OptionType::call, OptionType::put}) {
            input = terms(direction);
            input.strike = direction == OptionType::call ? 125.0 : 75.0;
            const auto excluded = engine.price(*make_double_barrier_option(input), market());
            REQUIRE(excluded);
            CHECK(*excluded == 0.0);
        }
        for (const bool up : {false, true}) {
            for (const auto timing : {RebateTiming::at_hit, RebateTiming::at_expiry}) {
                input = terms();
                input.lower_barrier = up ? 80.0 : 98.0;
                input.upper_barrier = up ? 102.0 : 120.0;
                input.rebate = 3.0;
                input.rebate_timing = timing;
                const double hit_time = std::log(up ? 102.0 / 100.0 : 100.0 / 98.0) / 0.03;
                const auto actual = engine.price(*make_double_barrier_option(input), market(100.0, start_of_day(start), 0.04, up ? 0.01 : 0.07, 1e-12));
                CAPTURE(up, timing);
                INFO((actual ? "tiny-volatility price returned" : actual.error().message));
                REQUIRE(actual);
                CHECK(*actual == Catch::Approx(3.0 * std::exp(-0.04 * (timing == RebateTiming::at_hit ? hit_time : 1.0))).margin(1e-7));
            }
        }
    });
}

TEST_CASE("Double barrier MC sampling thread step counts and concurrent calls reproduce", "[double-barrier-numerical]")
{
    const auto option = make_double_barrier_option(terms()).value();
    const auto context = market();
    const MonteCarloSettings defaults;
    const auto settings = MonteCarloDoubleBarrierEngine{}.settings();
    CHECK(settings.path_count == defaults.path_count);
    CHECK(settings.step_count == defaults.step_count);
    CHECK(settings.seed == defaults.seed);
    CHECK(settings.backend == defaults.backend);
    CHECK(settings.thread_count == defaults.thread_count);
    CHECK(settings.sampling == defaults.sampling);
    CHECK(settings.antithetic == defaults.antithetic);
    for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol}) {
        for (const bool paired : {false, true}) {
            MonteCarloSettings configured{.path_count = 1024, .seed = 17, .thread_count = 1, .sampling = sampling, .antithetic = paired};
            const auto engine = make_monte_carlo_double_barrier_engine(configured).value();
            const auto expected = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma}).value();
            configured.thread_count = 4;
            configured.step_count = 10'000;
            const auto parallel = make_monte_carlo_double_barrier_engine(configured).value();
            const auto actual = parallel.price_with_greeks(option, context, {Greek::delta, Greek::gamma}).value();
            CHECK(actual.price() == expected.price());
            CHECK(actual.values_view() == expected.values_view());
            auto other = std::async(std::launch::async, [&] { return parallel.price(option, context); });
            CHECK(*parallel.price(option, context) == expected.price());
            CHECK(*other.get() == expected.price());
        }
    }
    const auto invalid_paths = make_monte_carlo_double_barrier_engine({.path_count = 0});
    REQUIRE_FALSE(invalid_paths);
    CHECK(invalid_paths.error().category == ErrorCategory::invalid_parameter);
    const auto invalid_sobol = make_monte_carlo_double_barrier_engine({.path_count = 1000, .sampling = MonteCarloSampling::sobol});
    REQUIRE_FALSE(invalid_sobol);
    CHECK(invalid_sobol.error().category == ErrorCategory::invalid_parameter);
    const auto cuda_sobol = make_monte_carlo_double_barrier_engine({.path_count = 1024, .backend = MonteCarloBackend::cuda, .sampling = MonteCarloSampling::sobol});
    REQUIRE_FALSE(cuda_sobol);
    CHECK(cuda_sobol.error().category == ErrorCategory::unsupported_operation);
}

TEST_CASE("Double barrier MC odd antithetic path counts preserve in out payoff parity", "[double-barrier-numerical]")
{
    const auto context = market();
    for (const int paths : {1, 2, 3, 5, 257}) {
        const MonteCarloSettings settings{.path_count = paths, .seed = 42, .antithetic = true};
        const auto engine = make_monte_carlo_double_barrier_engine(settings).value();
        double expected = 0.0;
        for (int path = 0; path < (paths + 1) / 2; ++path) {
            detail::Pcg32 generator{42, static_cast<std::uint64_t>(path)};
            std::normal_distribution<double> normal;
            const double draw = normal(generator);
            const int sides = path < paths / 2 ? 2 : 1;
            for (int side = 0; side < sides; ++side)
                expected += std::max(100.0 * std::exp(0.01 + 0.2 * (side == 0 ? draw : -draw)) - 100.0, 0.0) * std::exp(-0.04);
        }
        const auto out = engine.price(*make_double_barrier_option(terms()), context);
        const auto in = engine.price(*make_double_barrier_option(terms(OptionType::call, DoubleBarrierType::knock_in)), context);
        REQUIRE(out);
        REQUIRE(in);
        CHECK(*in + *out == Catch::Approx(expected / paths).margin(1e-11));
    }
}

TEST_CASE("Double barrier unrepresentable prices return structured core errors", "[double-barrier-numerical]")
{
    const auto context = market(100.0, start_of_day(start), -1000.0, -1000.0);
    with_engines([&](const auto& engine) {
        const auto result = engine.price(*make_double_barrier_option(terms()), context);
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::invalid_result);
    });
}

TEST_CASE("Double barrier implied volatility recovers and rejects fixed remaining cashflows", "[double-barrier-numerical]")
{
    const auto unseeded = make_monte_carlo_double_barrier_engine({.path_count = 2048}).value();
    const auto seeded = detail::with_monte_carlo_seed<MonteCarloDoubleBarrierEngine, DoubleBarrierOption>(
        unseeded, [&](const auto& request) -> Result<bool> {
            CHECK(request.settings().seed.has_value());
            const auto option = make_double_barrier_option(terms()).value();
            return request.price(option, market()).value() == request.price(option, market()).value();
        });
    REQUIRE(seeded);
    CHECK(*seeded);
    with_engines([&](const auto& engine) {
        auto input = terms();
        input.lower_barrier = 50.0;
        input.upper_barrier = 200.0;
        const auto option = make_double_barrier_option(input).value();
        const auto quote = engine.price(option, market());
        REQUIRE(quote);
        const auto actual = implied_volatility(engine, option, market(), *quote, {.lower_bound = 0.15, .upper_bound = 0.25});
        REQUIRE(actual);
        CHECK(*actual == Catch::Approx(0.2).margin(1e-5));
        input.rebate = 3.0;
        input.touch_state = BarrierTouchState::touched;
        const auto fixed = make_double_barrier_option(input).value();
        const auto context = market(100.0, start_of_day(start) + std::chrono::days{1});
        const auto fixed_value = engine.price(fixed, context);
        REQUIRE(fixed_value);
        const auto rejected = implied_volatility(engine, fixed, context, *fixed_value);
        REQUIRE_FALSE(rejected);
        CHECK(rejected.error().category == ErrorCategory::unsupported_operation);
    });
}

TEST_CASE("Double barrier CUDA prices and Greeks match references or report unavailability", "[double-barrier-numerical][cuda]")
{
    const auto engine = make_monte_carlo_double_barrier_engine({.path_count = 262'144, .seed = 42, .backend = MonteCarloBackend::cuda, .antithetic = true}).value();
    for (const bool scheduled : {false, true}) {
        CAPTURE(scheduled);
        auto input = terms();
        input.rebate = 2.0;
        input.rebate_timing = RebateTiming::at_hit;
        if (scheduled) {
            input.observation_mode = ObservationMode::scheduled;
            input.observation_dates = {first, expiry};
        }
        const auto option = make_double_barrier_option(input).value();
        const auto actual = engine.price_with_greeks(option, market(), {Greek::delta, Greek::gamma});
        if (!actual && actual.error().category == ErrorCategory::backend_unavailable) {
            SUCCEED("CUDA unavailable; public failure category verified");
            return;
        }
        INFO("error category " << (actual ? 0 : static_cast<int>(actual.error().category)));
        INFO((actual ? "CUDA price returned" : actual.error().message));
        REQUIRE(actual);
        const auto reference = scheduled ? make_finite_difference_double_barrier_engine({1200, 1200})->price_with_greeks(option, market(), {Greek::delta, Greek::gamma})
                                         : AnalyticDoubleBarrierEngine{}.price_with_greeks(option, market(), {Greek::delta, Greek::gamma});
        REQUIRE(reference);
        CHECK(actual->price() == Catch::Approx(reference->price()).margin(0.12));
        CHECK(*actual->require(Greek::delta) == Catch::Approx(*reference->require(Greek::delta)).margin(0.05));
        CHECK(*actual->require(Greek::gamma) == Catch::Approx(*reference->require(Greek::gamma)).margin(0.02));
    }
}
