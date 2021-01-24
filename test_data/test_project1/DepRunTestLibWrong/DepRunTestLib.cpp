

#include"DepRunTestLib.h"
#include<iostream>

using namespace std;

void testFunction() {
	cout << "Hello CMake." << endl;
}

TestClass::TestClass() : y(0) {}

float TestClass::testMethod(float x) {
	return (x + y);
}

int TestClass::testStaticMethod(int z) {
	return z;
}