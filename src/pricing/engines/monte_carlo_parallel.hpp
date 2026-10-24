#pragma once

#include <algorithm>
#include <optional>
#include <vector>

#include <oneapi/tbb/parallel_for.h>
#include <oneapi/tbb/task_arena.h>
#include <oneapi/tbb/task_group.h>
#include <kiyosi/core/error.hpp>

namespace kiyosi::detail {

// Fixed blocks keep random streams and reduction order independent of concurrency.
inline constexpr int monte_carlo_block_size = 256;

inline int monte_carlo_block_count(int count)
{
    return (count + monte_carlo_block_size - 1) / monte_carlo_block_size;
}

// One synchronous request can reuse scheduling storage across exercise dates.
class MonteCarloBlocks {
public:
    MonteCarloBlocks(int count, int thread_count)
        : count_(count), blocks_(monte_carlo_block_count(count))
    {
        if (thread_count == 0)
            thread_count = std::min(blocks_, oneapi::tbb::this_task_arena::max_concurrency());
        if (thread_count != 1 && blocks_ != 1) {
            errors_.resize(static_cast<std::size_t>(blocks_));
            arena_.emplace(thread_count);
        }
    }

    template <typename Evaluate>
    Result<void> run(const Evaluate& evaluate)
    {
        const auto block_result = [&](int block) {
            return evaluate(block, block * monte_carlo_block_size,
                            std::min(count_, (block + 1) * monte_carlo_block_size));
        };
        if (!arena_) {
            for (int block = 0; block < blocks_; ++block) {
                const auto result = block_result(block);
                if (!result) return std::unexpected(result.error());
            }
            return {};
        }
        arena_->execute([&] {
            // An embedding application's cancellation must not silently omit pricing work.
            oneapi::tbb::task_group_context context{oneapi::tbb::task_group_context::isolated};
            oneapi::tbb::parallel_for(0, blocks_, [&](int block) {
                const auto result = block_result(block);
                errors_[static_cast<std::size_t>(block)] = result ? std::nullopt : std::optional{result.error()}; }, context);
        });
        for (const auto& error : errors_)
            if (error) return std::unexpected(*error);
        return {};
    }

private:
    int count_;
    int blocks_;
    std::vector<std::optional<Error>> errors_;
    std::optional<oneapi::tbb::task_arena> arena_;
};

template <typename Evaluate>
Result<void> monte_carlo_blocks(int count, int thread_count, const Evaluate& evaluate)
{
    return MonteCarloBlocks{count, thread_count}.run(evaluate);
}

} // namespace kiyosi::detail
