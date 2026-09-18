// Force a real final link against the prepared GStreamer import library.
extern "C" void gst_version(unsigned int*, unsigned int*, unsigned int*, unsigned int*);

int main() {
  unsigned int major = 0;
  unsigned int minor = 0;
  unsigned int micro = 0;
  unsigned int nano = 0;
  gst_version(&major, &minor, &micro, &nano);
  return major == 1 ? 0 : 1;
}
