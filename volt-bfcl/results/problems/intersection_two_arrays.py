def intersect(nums1, nums2):
    return list(set(nums1) & set(nums2))

print(sorted(intersect([1,2,2,1], [2,2])))
print(sorted(intersect([4,9,5], [9,4,9,8,4])))