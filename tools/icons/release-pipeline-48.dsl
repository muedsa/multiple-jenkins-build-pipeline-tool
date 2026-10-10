<!-- Release Pipeline icon - 48x48
     介于 64 与 32 之间的过渡尺寸，单独设计而非缩放：
       - 保留全部三个阶段柱（比 32px 多一个阶段，但提高阶段 1 的透明度补偿）
       - 箭头描边取 64 与 32 之间的值，杆长加长以免缩进箭翼
     构造法与主图一致。尖端 T=(37.5, 11)，描边 S=3.8，翼长 Lw=9.5，杆长 Ls=11.5。
     箭杆定位（绕中心顺时针 45°，上端中心落在 T）：
         left = T.x - 0.3536*Ls - S/2
         top  = T.y + 0.3536*Ls - Ls/2
     竖翼顶部比尖端上探 1px 闭合接缝。 -->
<Snapshot type="png" background="transparent">
    <Container
        width="48" height="48"
        borderRadius="11"
        gradientType="LINEAR"
        gradientColors="#0B1B4D,#1E3A8A,#5B21B6"
        gradientStops="0,0.55,1"
        gradientBegin="TOP_LEFT"
        gradientEnd="BOTTOM_RIGHT">

        <Stack>

            <!-- 阶段 1：已完成（小尺寸下提高透明度补偿） -->
            <Positioned left="9" bottom="9" width="7" height="8.5">
                <Container color="#38BDF87D" borderRadius="2.2"/>
            </Positioned>

            <!-- 阶段 2：进行中 -->
            <Positioned left="20" bottom="9" width="7" height="13">
                <Container color="#22D3EEE6" borderRadius="2.2"/>
            </Positioned>

            <!-- 阶段 3：发布中 -->
            <Positioned left="31" bottom="9" width="7" height="18">
                <Container color="#67E8F9" borderRadius="2.2"/>
            </Positioned>

            <!-- 箭头横翼：从尖端向左 -->
            <Positioned left="28" top="9.1" width="9.5" height="3.8">
                <Container color="#F0F9FF" borderRadius="1.9"/>
            </Positioned>

            <!-- 箭头竖翼：从尖端向下，顶部上探 1px 闭合接缝 -->
            <Positioned left="35.6" top="10" width="3.8" height="10.5">
                <Container color="#F0F9FF" borderRadius="1.9"/>
            </Positioned>

            <!-- 箭杆：旋转 45°，上端落在尖端 -->
            <Positioned left="31.56" top="10.07" width="3.8" height="11.5">
                <Transform
                    matrix="(0.7071,0.7071,0,0,-0.7071,0.7071,0,0,0,0,1,0,0,0,0,1)"
                    alignment="CENTER">
                    <Container width="3.8" height="11.5" color="#F0F9FF" borderRadius="1.9"/>
                </Transform>
            </Positioned>

        </Stack>
    </Container>
</Snapshot>