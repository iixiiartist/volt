def move_zeroes(nums):
    non_zero = [n for n in nums if n != 0]
    nums[:] = non_zero + [0] * (len(nums) - len(non_zero))

nums = [0,1,0,3,12]
move_zeroes(nums)
print(nums)
nums2 = [0]
move_zeroes(nums2)
print(nums2)