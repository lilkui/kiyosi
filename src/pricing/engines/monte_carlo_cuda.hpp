#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>

#include <kiyosi/pricing/detail/autocallable_program.hpp>

#include "monte_carlo_discount.hpp"

namespace kiyosi::detail {

enum class CudaPricingStatus : unsigned char {
    success,
    unavailable,
    failure,
    out_of_memory,
    invalid_result,
};

struct CudaEuropeanRequest {
    int path_count;
    std::uint64_t seed;
    double spot;
    double strike;
    double drift;
    double diffusion;
    int payoff_sign;
};

struct CudaAmericanRequest {
    int path_count;
    int step_count;
    std::uint64_t seed;
    double spot;
    double strike;
    double drift;
    double diffusion;
    double discount;
    int payoff_sign;
};

struct CudaSimulationStep {
    double drift;
    double diffusion;
    MonteCarloDiscount discount;
};

struct CudaAutocallableStep {
    CudaSimulationStep simulation;
    AutocallableEvent event;
    double alternate_coupon{};
};

struct CudaAccumulatorRequest {
    int path_count;
    std::uint64_t seed;
    double spot;
    double strike;
    double knock_out;
    double daily_quantity;
    double acceleration;
    double initial_quantity;
};

struct CudaAutocallableRequest {
    int path_count;
    std::uint64_t seed;
    double spot;
    MonteCarloDiscount terminal_discount;
    AutocallableProgram program;
    AutocallablePathState initial_state;
    AutocallableProgram alternate_program{};
    AutocallablePathState alternate_initial_state{};
};

struct CudaPricingResult {
    CudaPricingStatus status;
    double payoff_mean;
    const char* message;
    double alternate_payoff_mean{};
};

// One operation owns this workspace; it must not be shared across concurrent calls.
// Resources are created lazily so settlements still work without a CUDA device.
class CudaWorkspace {
public:
    struct Impl;
    CudaWorkspace();
    ~CudaWorkspace();
    CudaWorkspace(const CudaWorkspace&) = delete;
    CudaWorkspace& operator=(const CudaWorkspace&) = delete;
    Impl& storage();

private:
    std::unique_ptr<Impl> impl_;
};

[[nodiscard]] CudaPricingResult cuda_european_price(CudaEuropeanRequest request, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaPricingResult cuda_american_price(CudaAmericanRequest request, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaPricingResult cuda_accumulator_price(
    CudaAccumulatorRequest request, std::span<const CudaSimulationStep> steps, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaPricingResult cuda_autocallable_price(
    CudaAutocallableRequest request, std::span<const CudaAutocallableStep> steps, CudaWorkspace* workspace = nullptr);
[[nodiscard]] CudaPricingResult cuda_autocallable_coupon_prices(
    CudaAutocallableRequest request, std::span<const CudaAutocallableStep> steps, CudaWorkspace* workspace = nullptr);

} // namespace kiyosi::detail
