def max_subarray(nums):
    max_sum = float('-inf')
    current_sum = 0
    start = 0
    end = 0
    temp_start = 0
    for i, n in enumerate(nums):
        if current_sum <= 0:
            current_sum = n
            temp_start = i
        else:
            current_sum += n
        if current_sum > max_sum:
            max_sum = current_sum
            start = temp_start
            end = i
    return nums[start:end+1]

print(max_subarray([-2,1,-3,4,-1,2,1,-5,4]))
print(max_subarray([1]))
print(max_subarray([5,4,-1,7,8]))