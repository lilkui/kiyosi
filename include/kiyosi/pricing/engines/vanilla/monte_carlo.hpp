#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

struct MonteCarloSettings {
    int path_count = 100'000;
    int step_count = 50;
    std::optional<std::uint64_t> seed;
};


class KIYOSI_EXPORT MonteCarloEuropeanEngine {
public:
    explicit MonteCarloEuropeanEngine(MonteCarloSettings settings = {}) : settings_(settings) {}
    MonteCarloEuropeanEngine(int path_count, int step_count,
                             std::optional<std::uint64_t> seed = std::nullopt)
        : settings_{path_count, step_count, seed} {}

    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption& option, const PricingContext& context) const;

    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    MonteCarloSettings settings_;
};

class KIYOSI_EXPORT MonteCarloAmericanEngine {
public:
    explicit MonteCarloAmericanEngine(MonteCarloSettings settings = {}) : settings_(settings) {}
    MonteCarloAmericanEngine(int path_count, int step_count,
                             std::optional<std::uint64_t> seed = std::nullopt)
        : settings_{path_count, step_count, seed} {}

    [[nodiscard]] result<PricingResult> price(
        const AmericanOption& option, const PricingContext& context) const;

    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    MonteCarloSettings settings_;
};


} // namespace kiyosi
