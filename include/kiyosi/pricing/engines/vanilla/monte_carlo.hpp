#pragma once

#include <concepts>
#include <cstdint>
#include <optional>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

/// Monte Carlo valuation for vanilla European and American options.
class KIYOSI_EXPORT MonteCarloVanillaEngine {
public:
    /// Creates an engine with aggregate Monte Carlo settings.
    explicit MonteCarloVanillaEngine(MonteCarloSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit path count, step count, seed, and backend.
    MonteCarloVanillaEngine(
        int path_count, int step_count, std::optional<std::uint64_t> seed = MonteCarloSettings{}.seed,
        MonteCarloBackend backend = MonteCarloSettings{}.backend)
        : settings_{path_count, step_count, seed, backend} {}

    /// Prices a European or American vanilla option.
    /// @return Pricing measures, or a contract, context, settings, or backend error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> &&
                 (std::same_as<Exercise, EuropeanExercise> || std::same_as<Exercise, AmericanExercise>)
    [[nodiscard]] Result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Exercise, EuropeanExercise>) return price_european(option, context);
        else return price_american(option, context);
    }

    /// Returns the engine settings.
    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_european(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price_american(const AmericanOption&, const PricingContext&) const;
    MonteCarloSettings settings_;
};

} // namespace kiyosi
